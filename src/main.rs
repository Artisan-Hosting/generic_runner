use crate::global_child::{
    init_child, init_monitor, replace_child, GLOBAL_CHILD,
    GLOBAL_MONITOR,
};
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

use dir_watcher::{MonitorMode, Options, RawFileMonitor, RecursiveMode};
use dusa_collection_utils::{
    core::errors::{ErrorArrayItem, Errors},
    core::logger::LogLevel,
    core::types::pathtype::PathType,
    log,
};
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
mod global_child;
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
    child.monitor_stdx().await;
    child.monitor_usage().await;
    init_child(child.clone().await).await;

    let mut change_count = 0;
    let trigger_count = settings.changes_needed;
    state.status = Status::Running;
    update_state(&mut state, &state_path, None).await;

    // Start monitoring the directory and get the asynchronous receiver
    log!(LogLevel::Trace, "Starting directory monitoring...");
    let options: Options = Options::default()
        .set_mode(RecursiveMode::Recursive)
        .set_monitor_mode(MonitorMode::Modify)
        .add_ignored_dirs(settings.ignored_paths())
        .set_target_dir(settings.safe_path())
        .set_interval(settings.interval_seconds.into())
        .set_validation(true);

    let monitor: RawFileMonitor = RawFileMonitor::new(options.clone()).await;
    monitor.start().await;

    // let mut event_rx = match get_event_reciver().await {
    //     Ok(recv) => recv,
    //     Err(_) => {
    //         log!(LogLevel::Warn, "File monitor in a weird state, re-initializing");
    //         let monitor: RawFileMonitor = RawFileMonitor::new(options).await;
    //         replace_monitor(monitor).await;
    //         match get_event_reciver().await {
    //             Ok(recv) => recv,
    //             Err(_) => {
    //                 log_error(&mut state, ErrorArrayItem::new(Errors::GeneralError, "Failed to start folder monitor"), &state_path).await;
    //                 wind_down_state(&mut state, &state_path).await;
    //                 std::process::exit(100);
    //             },
    //         }
    //     },
    // };

    let mut event_rx = match monitor.subscribe().await {
        Some(rx) => rx,
        None => {
            log!(LogLevel::Error, "Failed to subscribe to the dir monitor");
            state.error_log.push(ErrorArrayItem::new(
                Errors::GeneralError,
                "Failed to subscribe to the dir monitor",
            ));
            wind_down_state(&mut state, &state_path).await;
            std::process::exit(100);
        }
    };

    init_monitor(monitor).await;

    log!(LogLevel::Trace, "Entering main loop...");
    state.status = Status::Running;
    update_state(&mut state, &state_path, None).await;
    loop {
        tokio::select! {
            Ok(event) = event_rx.recv() => {
                log!(LogLevel::Trace, "Received directory change event: {:?}", event);
                change_count += 1;
                log!(LogLevel::Info, "Change detected: {} out of {}", change_count, trigger_count);
                log!(LogLevel::Debug, "Event details: {:?}", event);

                if change_count >= trigger_count {
                    if let Some(monitor) = GLOBAL_MONITOR.lock().await.as_mut() {
                        monitor.pause();
                    }

                    // monitor;
                    log!(LogLevel::Info, "Reached {} changes, handling event", trigger_count);
                    state.event_counter += 1;
                    state.status = Status::Building;
                    update_state(&mut state, &state_path, None).await;
                    log!(LogLevel::Info, "Killing the child");

                    if let Some(child) = GLOBAL_CHILD.lock().await.as_mut() {
                        if let Err(err) = child.kill().await {
                            log!(LogLevel::Error, "Error killing child: {}, requesting reload", err.err_mesg);
                            reload.store(true, Ordering::Relaxed);
                        }
                    }

                    // { // This coupled with kill_on_drop ensures that even if we don't properly kill the application it get's nuked
                    //     let mut _raw_child = GLOBAL_CHILD.lock().await.as_mut();
                    //     _raw_child = None;
                    //     sleep(Duration::from_millis(20)).await;
                    // }

                    if !child.running().await {
                        log!(LogLevel::Info, "Killed the child!");
                    }

                    // Spawn child process
                    log!(LogLevel::Trace, "Running one shot pre child");
                    if settings.build_command.is_some() {
                        log!(LogLevel::Info, "Running build step");
                        if let Err(err) = run_one_shot_process(&settings, &mut state, &state_path).await {
                            log!(LogLevel::Error, "One-shot process failed: {}", err);
                            log_error(&mut state, err, &state_path).await;
                            return;
                        }
                    }

                    replace_child(create_child(&mut state, &state_path, &settings).await).await;
                    if let Some(child) = GLOBAL_CHILD.lock().await.as_mut() {
                        child.monitor_stdx().await;
                        child.monitor_usage().await;
                    };

                    if let Some(monitor) = GLOBAL_MONITOR.lock().await.as_mut() {
                        monitor.resume();
                    }
                    change_count = 0; // Reset count
                }
            }
            _ = tokio::time::sleep(Duration::from_secs(5)) => {
                log!(LogLevel::Trace, "Periodic task triggered - checking child process status...");

                let mut respawn_child = false;

                // Getting stds from child and cheking it's pulse
                if let Some(child) = GLOBAL_CHILD.lock().await.as_mut() {
                    // Getting the stds out

                    { // Standard Out
                        let current_std_out = if let Ok(stdout) = child.get_std_out().await {
                            stdout
                        } else {
                            Vec::new()
                        };

                        if !current_std_out.is_empty() {
                            let new_values: Vec<(u64, String)> = current_std_out
                                .into_iter()
                                .filter(|val| !state.stdout.contains(val))
                                .collect();

                            state.stdout.extend(new_values);
                            state.stdout.sort_by_key(|val| val.0);
                            state.stdout.dedup();
                        }
                    }

                    { // Standard Err
                        let current_std_err = if let Ok(stderr) = child.get_std_err().await {
                            stderr
                        } else {
                            Vec::new()
                        };

                        if !current_std_err.is_empty() {
                            let new_values: Vec<(u64, String)> = current_std_err
                                .into_iter()
                                .filter(|val| !state.stderr.contains(val))
                                .collect();

                            state.stderr.extend(new_values);
                            state.stderr.sort_by_key(|val| val.0);
                            state.stderr.dedup();
                        }
                    }

                    if !child.running().await {
                        respawn_child = true;
                    }
                } else {
                    log!(LogLevel::Warn, "Failed to lock child for periodic checks skipping");
                }

                // Handling re-spawning child.
                if respawn_child {
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

                    replace_child(create_child(&mut state, &state_path, &settings).await).await;
                    if let Some(child) = GLOBAL_CHILD.lock().await.as_mut() {
                        child.monitor_stdx().await;
                        child.monitor_usage().await;
                    };

                    // logging
                    let message = "New child process spawned";
                    log!(LogLevel::Info, "{message}");
                    state.data = message.to_string();
                    state.status = Status::Running;
                    update_state(&mut state, &state_path, None).await;
                }


                // Cleaning up the state file
                state.error_log.dedup();
                if state.error_log.len() >= 5 {
                    state.error_log.remove(0);
                }

                { // Collecting metrics data to add to state
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
            replace_child(create_child(&mut state, &state_path, &settings).await).await;
            if let Some(child) = GLOBAL_CHILD.lock().await.as_mut() {
                child.monitor_stdx().await;
                child.monitor_usage().await;
            };

            log!(LogLevel::Info, "New child process spawned.");
            reload.store(false, Ordering::Relaxed);
        }

        if exit_graceful.load(Ordering::Relaxed) {
            log!(LogLevel::Debug, "Exiting gracefully");
            match timeout(Duration::from_secs(5), child.kill()).await {
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
