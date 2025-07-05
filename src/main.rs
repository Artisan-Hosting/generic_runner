use artisan_middleware::{
    aggregator::Status,
    config::AppConfig,
    dusa_collection_utils::{
        self,
        core::logger::{get_log_level, set_log_level},
    },
    process_manager::SupervisedChild,
    state_persistence::{log_error, update_state, wind_down_state, AppState, StatePersistence},
};
use child::{create_child, run_install_process, run_one_shot_process};
use config::{generate_application_state, get_config, specific_config};
use dusa_collection_utils::{
    core::errors::{ErrorArrayItem, Errors},
    core::logger::LogLevel,
    core::types::pathtype::PathType,
    log,
};
use monitor::monitor_directory;
use signals::{sighup_watch, sigusr_watch};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::time::timeout;

mod child;
mod config;
mod monitor;
mod signals;

#[tokio::main]
async fn main() {
    // Initialization
    log!(LogLevel::Trace, "Initializing application...");
    let mut config: AppConfig = get_config();
    let state_path: PathType = StatePersistence::get_state_path(&config);

    log!(LogLevel::Trace, "Loading specific configuration...");
    let settings = match specific_config() {
        Ok(loaded_data) => {
            log!(
                LogLevel::Trace,
                "Loaded specific configuration successfully"
            );
            loaded_data
        }
        Err(e) => {
            log!(LogLevel::Error, "Error loading settings: {}", e);
            std::process::exit(0)
        }
    };

    // Setting up the state of the application
    log!(LogLevel::Trace, "Setting up the application state...");
    let mut state: AppState = generate_application_state(&state_path, &config).await;

    // Listening for the sighup
    let reload: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let exit_graceful: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));

    sighup_watch(reload.clone());
    sigusr_watch(exit_graceful.clone());

    log!(LogLevel::Trace, "Setting state as active...");
    update_state(&mut state, &state_path, None).await;

    if config.debug_mode {
        log!(LogLevel::Info, "Application State: {}", state);
        log!(LogLevel::Info, "Application State: {}", settings);
        log!(LogLevel::Info, "Log Level: {}", config.log_level);
    }

    log!(LogLevel::Info, "{} Started", config.app_name);
    log!(
        LogLevel::Info,
        "Directory Monitoring: {}",
        settings.safe_path()
    );

    state.status = Status::Building;
    update_state(&mut state, &state_path, None).await;
    if settings.install_command.is_some() {
        log!(LogLevel::Trace, "Running install step");
        if let Err(err) = run_install_process(&settings, &mut state, &state_path).await {
            log!(LogLevel::Error, "{}", err)
        }
    }

    // Spawn child process
    log!(LogLevel::Trace, "Running one shot pre child");
    if settings.build_command.is_some() {
        log!(LogLevel::Trace, "Running build step");
        if let Err(err) = run_one_shot_process(&settings, &mut state, &state_path).await {
            log!(LogLevel::Error, "One-shot process failed: {}", err);
            log_error(&mut state, err, &state_path).await;
            return;
        }
    }

    log!(LogLevel::Trace, "Spawning child process...");
    let mut child: SupervisedChild = create_child(&mut state, &state_path, &settings).await;
    let mut change_count = 0;
    let trigger_count = settings.changes_needed;
    state.status = Status::Running;
    update_state(&mut state, &state_path, None).await;

    // Start monitoring the directory and get the asynchronous receiver
    log!(LogLevel::Trace, "Starting directory monitoring...");
    let mut event_rx = match monitor_directory(settings.safe_path(), settings.ignored_paths()).await
    {
        Ok(receiver) => {
            log!(LogLevel::Trace, "Successfully started directory monitoring");
            receiver
        }
        Err(err) => {
            log!(LogLevel::Error, "Watcher error: {}", err);
            state
                .error_log
                .push(ErrorArrayItem::new(Errors::GeneralError, err.to_string()));
            wind_down_state(&mut state, &state_path).await;
            std::process::exit(0);
        }
    };

    log!(LogLevel::Trace, "Entering main loop...");
    state.status = Status::Running;
    update_state(&mut state, &state_path, None).await;
    loop {
        tokio::select! {
            Some(event) = event_rx.recv() => {
                log!(LogLevel::Trace, "Received directory change event: {:?}", event);
                change_count += 1;
                log!(LogLevel::Info, "Change detected: {} out of {}", change_count, trigger_count);
                log!(LogLevel::Debug, "Event details: {:?}", event);

                if change_count >= trigger_count {
                    log!(LogLevel::Info, "Reached {} changes, handling event", trigger_count);
                    state.event_counter += 1;
                    state.status = Status::Building;
                    update_state(&mut state, &state_path, None).await;
                    log!(LogLevel::Info, "Killing the child");

                    match child.clone().await.kill().await {
                        Ok(_) => {
                            // creating new child
                            log!(LogLevel::Trace, "Spawning child process...");
                            child = create_child(&mut state, &state_path, &settings).await;
                            log!(LogLevel::Debug, "New child process spawned: {}", child.get_pid().await.unwrap());
                        },
                        Err(error) => {
                            log!(LogLevel::Error, "Failed to wait for child process termination: {}", error);
                            log_error(&mut state, error, &state_path).await;
                        },
                    }

                    change_count = 0; // Reset count
                }
            }
            _ = tokio::time::sleep(Duration::from_secs(5)) => {
                log!(LogLevel::Trace, "Periodic task triggered - checking child process status...");

                // Getting the stds out
                match child.get_std_out().await {
                    Ok(mut stdvec) => {
                        state.stdout.append(&mut stdvec);
                    },
                    Err(err) => {
                        log!(LogLevel::Error, "Failed to get standart out: {}", err.err_mesg)
                    },
                }

                match child.get_std_err().await {
                    Ok(mut errvec) => {
                        state.stderr.append(&mut errvec);
                    },
                    Err(err) => {
                        log!(LogLevel::Error, "Failed to get standart error: {}", err.err_mesg)
                    },
                }

                if !child.running().await {
                    log!(LogLevel::Warn, "Child process {:?} is not running. Restarting...", child.get_pid().await);

                    if let Ok(_) = child.kill().await {
                        log!(LogLevel::Info, "Executed the previous child")
                    }

                    if settings.build_command.is_some() {
                        if let Err(err) = run_one_shot_process(&settings, &mut state, &state_path).await {
                            log!(LogLevel::Error, "One-shot process failed: {}", err);
                            log_error(&mut state, err, &state_path).await;
                            return;
                        }
                    }

                    log!(LogLevel::Info, "One shot finished, Spawning new child");

                    child = create_child(&mut state, &state_path, &settings).await;
                    child.monitor_stdx().await;
                    child.monitor_usage().await;
                    let message = "New child process spawned";

                    log!(LogLevel::Info, "{message}");
                    state.data = message.to_string();
                    state.status = Status::Running;
                    update_state(&mut state, &state_path, None).await;
                }

                // this is just a trimming function to limit the upstream communications
                if state.error_log.len() >= 3 { // * Change this limit dependent on the project
                    state.error_log.remove(0);
                    state.error_log.dedup();
                }

                // Update state as needed
                state.data = String::from("Nominal");
                if let Ok(metrics) = child.get_metrics().await {
                    // Ensuring we are within the specified limits
                    if metrics.memory_usage >= state.config.max_ram_usage as f64 {
                        state.error_log.push(ErrorArrayItem::new(Errors::OverRamLimit, "Application has exceeded ram limit"))
                    }

                    state.status = Status::Running;
                    update_state(&mut state, &state_path, Some(metrics)).await;
                } else {
                    state.data = String::from("Failed to get metric data");
                    state.error_log.push(ErrorArrayItem::new(Errors::GeneralError, "Failed to get metric data from the child"));
                    state.status = Status::Warning;
                    update_state(&mut state, &state_path, None).await;
                }


            }

            _ = tokio::signal::ctrl_c() => {
                log!(LogLevel::Info, "CTRL + C recieved");
                exit_graceful.store(true, Ordering::Relaxed);
            }
        }

        if reload.load(Ordering::Relaxed) {
            log!(LogLevel::Debug, "Reloading");

            // reload config file
            config = get_config();

            // Updating state data
            state = generate_application_state(&state_path, &config).await;

            // Killing and redrawing the process
            if let Err(err) = child.kill().await {
                log_error(&mut state, err, &state_path).await;
                wind_down_state(&mut state, &state_path).await;
                // We're in a weird state kys and let systemd try again.
                std::process::exit(100)
            }

            // running one shot again if configured
            if settings.build_command.is_some() {
                if let Err(err) = run_one_shot_process(&settings, &mut state, &state_path).await {
                    log!(LogLevel::Error, "One-shot process failed: {}", err);
                    log_error(&mut state, err, &state_path).await;
                    return;
                }
            }

            // creating new service
            child = create_child(&mut state, &state_path, &settings).await;
            child.monitor_stdx().await;
            child.monitor_usage().await;
            log!(LogLevel::Info, "New child process spawned.");

            reload.store(false, Ordering::Relaxed);
        }

        if exit_graceful.load(Ordering::Relaxed) {
            log!(LogLevel::Debug, "Exiting gracefully");
            match timeout(Duration::from_secs(3), child.kill()).await {
                Ok(execution_result) => match execution_result {
                    Ok(_) => {
                        state.status = Status::Stopping;
                        wind_down_state(&mut state, &state_path).await;
                        std::process::exit(0);
                    }
                    Err(err) => {
                        state.status = Status::Stopping;
                        log!(LogLevel::Error, "{}", err);
                        state.error_log.push(err);
                        wind_down_state(&mut state, &state_path).await;
                        std::process::exit(100);
                    }
                },
                Err(err) => {
                    log!(LogLevel::Error, "{}", err);
                    log!(LogLevel::Error, "We hit the timeout while gracefully shutting down. You might have to run systemctl kill ais_xxx to ensure you start correctly nextime");
                    log_error(
                        &mut state,
                        ErrorArrayItem::new(Errors::TimedOut, err.to_string()),
                        &state_path,
                    )
                    .await;
                    wind_down_state(&mut state, &state_path).await;
                    std::process::exit(100);
                }
            }
        }

        if state.config.debug_mode {
            let log_level = get_log_level();
            set_log_level(LogLevel::Trace);
            log!(LogLevel::Trace, "printing std out");
            for lines in &state.stdout {
                log!(LogLevel::Debug, "{}", lines.1);
            }
            set_log_level(log_level);
        }
    }
}
