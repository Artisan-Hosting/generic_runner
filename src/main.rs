use artisan_middleware::{
    common::{log_error, update_state},
    config::AppConfig,
    log,
    logger::{set_log_level, LogLevel},
    process_manager::ProcessManager,
    state_persistence::{AppState, StatePersistence},
    timestamp::current_timestamp,
};
use child::{create_child, run_one_shot_process};
use config::{get_config, specific_config,  wind_down_state};
use dusa_collection_utils::{
    errors::{ErrorArrayItem, Errors},
    rwarc::LockWithTimeout,
    types::PathType,
};
use monitor::monitor_directory;
use nix::libc::{killpg, SIGKILL};
use std::{io, time::Duration};

mod child;
mod config;
mod monitor;

#[tokio::main]
async fn main() {
    // Initialization
    log!(LogLevel::Trace, "Initializing application...");
    let config: AppConfig = get_config();
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
    let mut state: AppState = match StatePersistence::load_state(&state_path) {
        Ok(mut loaded_data) => {
            log!(LogLevel::Info, "Loaded previous state data");
            log!(LogLevel::Trace, "Previous state data: {:#?}", loaded_data);
            loaded_data.is_active = false;
            loaded_data.data = String::from("Initializing");
            loaded_data.config.debug_mode = config.debug_mode;
            loaded_data.last_updated = current_timestamp();
            loaded_data.config.log_level = config.log_level;
            set_log_level(loaded_data.config.log_level);
            loaded_data.error_log.clear();
            update_state(&mut loaded_data, &state_path);
            loaded_data
        }
        Err(e) => {
            log!(LogLevel::Warn, "No previous state loaded, creating new one");
            log!(LogLevel::Debug, "Error loading previous state: {}", e);
            let mut state = AppState {
                data: String::new(),
                last_updated: current_timestamp(),
                event_counter: 0,
                is_active: false,
                error_log: vec![],
                config: config.clone(),
            };
            state.is_active = false;
            state.data = String::from("Initializing");
            state.config.debug_mode = config.debug_mode;
            state.last_updated = current_timestamp();
            state.config.log_level = config.log_level;
            set_log_level(state.config.log_level);
            state.error_log.clear();
            update_state(&mut state, &state_path);

            state
        }
    };

    log!(LogLevel::Trace, "Setting state as active...");
    state.is_active = true;
    update_state(&mut state, &state_path);

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

    // Spawn child process
    log!(LogLevel::Trace, "Running one shot pre child");
    // Run the one-shot process before creating the child
    if let Err(err) = run_one_shot_process(&settings).await {
        log!(LogLevel::Error, "One-shot process failed: {}", err);
        let error = ErrorArrayItem::new(Errors::GeneralError, err);
        log_error(&mut state, error, &state_path);
        return;
    }

    log!(LogLevel::Trace, "Spawning child process...");
    let child: LockWithTimeout<tokio::process::Child> =
        LockWithTimeout::new(create_child(&mut state, &state_path, &settings).await);

    match child.try_read().await {
        Ok(process) => {
            if let Some(pid) = process.id() {
                log!(LogLevel::Info, "Child spawned: {}", pid);
                log!(LogLevel::Trace, "Child process info: {:?}", process);
                state.data = format!("Child spawned: {}", pid);
                update_state(&mut state, &state_path);
            } else {
                log!(
                    LogLevel::Error,
                    "Failed to get child process ID after spawning"
                );
                std::process::exit(0);
            }
        }
        Err(err) => {
            log!(LogLevel::Error, "Failed to spawn child process: {}", err);
            let error = ErrorArrayItem::new(Errors::GeneralError, err.to_string());
            log_error(&mut state, error, &state_path);
            std::process::exit(0);
        }
    };

    let mut change_count = 0;
    let trigger_count = settings.changes_needed;

    // Start monitoring the directory and get the asynchronous receiver
    log!(LogLevel::Trace, "Starting directory monitoring...");
    let mut event_rx = match monitor_directory(settings.safe_path()).await {
        Ok(receiver) => {
            log!(LogLevel::Trace, "Successfully started directory monitoring");
            receiver
        }
        Err(err) => {
            log!(LogLevel::Error, "Watcher error: {}", err);
            wind_down_state(&mut state, &state_path);
            std::process::exit(0);
        }
    };

    log!(LogLevel::Trace, "Entering main loop...");
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
                    update_state(&mut state, &state_path);
                    log!(LogLevel::Info, "Killing the child");

                    // Acquire a write lock to modify the child process
                    if let Ok(mut child) = child.try_write_with_timeout(None).await {
                        if let Some(pid) = child.id() {
                            log!(LogLevel::Trace, "Attempting to kill child process with ID: {}", pid);

                            // Kill the entire process group
                            unsafe {
                                let pgid = pid; // Since we set pgid to pid in pre_exec
                                if killpg(pgid as i32, SIGKILL) == -1 { // apperently in C -1 is the errono ?
                                    let err = io::Error::last_os_error();
                                    log!(LogLevel::Error, "Failed to kill child process group: {}", err);
                                    let error = ErrorArrayItem::new(Errors::GeneralError, err.to_string());
                                    log_error(&mut state, error, &state_path);
                                    continue;
                                }
                            }

                            // Wait for the child to be fully terminated
                            match child.wait().await {
                                Ok(status) => {
                                    log!(LogLevel::Trace, "Child process terminated with status: {:?}", status);

                                    log!(LogLevel::Trace, "Running one shot before re-creating child");
                                    // Run the one-shot process before creating the child
                                     if let Err(err) = run_one_shot_process(&settings).await {
                                         log!(LogLevel::Error, "One-shot process failed: {}", err);
                                         let error = ErrorArrayItem::new(Errors::GeneralError, err);
                                         log_error(&mut state, error, &state_path);
                                         return;
                                     }
                                    log!(LogLevel::Info, "One shot finished, Spawning new child");

                                    *child = create_child(&mut state, &state_path, &settings).await;
                                    log!(LogLevel::Info, "New child process spawned.");
                                },
                                Err(err) => {
                                    log!(LogLevel::Error, "Failed to wait for child process termination: {}", err);
                                    let error = ErrorArrayItem::new(Errors::GeneralError, err.to_string());
                                    log_error(&mut state, error, &state_path);
                                }
                            }
                        } else {
                            log!(LogLevel::Error, "Child process ID not available during kill attempt");
                        }
                    } else {
                        log!(LogLevel::Error, "Error acquiring write lock for child process");
                        continue;
                    }

                    change_count = 0; // Reset count
                }
            }
            _ = tokio::time::sleep(Duration::from_secs(3)) => {
                log!(LogLevel::Trace, "Periodic task triggered - checking child process status...");

                // Acquire a read lock to check the status of the child process
                if let Ok(child_process) = child.try_read_with_timeout(None).await {
                    if let Some(child_id) = child_process.id() {
                        log!(LogLevel::Trace, "Checking if child process {} is running...", child_id);

                        if !ProcessManager::is_process_running(child_id as i32) {
                            log!(LogLevel::Warn, "Child process {} is not running. Restarting...", child_id);
                            if let Ok(mut child) = child.try_write_with_timeout(None).await {
                                // Run the one-shot process before creating the child
                                if let Err(err) = run_one_shot_process(&settings).await {
                                    log!(LogLevel::Error, "One-shot process failed: {}", err);
                                    let error = ErrorArrayItem::new(Errors::GeneralError, err);
                                    log_error(&mut state, error, &state_path);
                                    return;
                                }
                                log!(LogLevel::Info, "One shot finished, Spawning new child");

                                *child = create_child(&mut state, &state_path, &settings).await;
                                log!(LogLevel::Info, "New child process spawned.");
                            } else {
                                log!(LogLevel::Error, "Error acquiring write lock for child process restart");
                            }
                        } else {
                            log!(LogLevel::Debug, "Child process {} is still running.", child_id);
                        }
                    } else {
                        log!(LogLevel::Error, "Failed to get child process ID during status check");
                    }
                } else {
                    log!(LogLevel::Error, "Error while trying to read the child process status");
                    let error = ErrorArrayItem::new(Errors::GeneralError, "Failed to read child process status".to_string());
                    log_error(&mut state, error, &state_path);
                    wind_down_state(&mut state, &state_path);
                    break;
                }

                // Update state as needed
                state.is_active = true;
                state.data = String::from("Nominal");
                update_state(&mut state, &state_path);
            }
        }
    }
}
