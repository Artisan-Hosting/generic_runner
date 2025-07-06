use artisan_middleware::dusa_collection_utils::core::errors::Errors;
use artisan_middleware::dusa_collection_utils::core::functions::current_timestamp;
use artisan_middleware::dusa_collection_utils::log;
use artisan_middleware::process_manager::{
    spawn_complex_process, spawn_simple_process, SupervisedChild,
};
use artisan_middleware::state_persistence::{log_error, update_state, wind_down_state};
use artisan_middleware::{
    dusa_collection_utils::{
        core::errors::ErrorArrayItem, core::logger::LogLevel, core::types::pathtype::PathType,
    },
    state_persistence::AppState,
};
use shell_words::split;
use std::fs;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::config::AppSpecificConfig;

pub async fn create_child(
    mut state: &mut AppState,
    state_path: &PathType,
    settings: &AppSpecificConfig,
) -> SupervisedChild {
    log!(LogLevel::Trace, "Creating child process...");

    let parts = split(&settings.run_command).unwrap_or_else(|_| {
        settings
            .run_command
            .split_whitespace()
            .map(|s| s.to_string())
            .collect()
    });
    let mut iter = parts.into_iter();
    let program = iter.next().unwrap();
    let mut command: Command = Command::new(program);
    for arg in iter {
        command.arg(arg);
    }

    match spawn_complex_process(&mut command, Some(settings.project_path()), false, true).await {
        Ok(mut spawned_child) => {
            // initialize monitor loop.
            spawned_child.monitor_usage().await;
            spawned_child.monitor_stdx().await;
            // read the pid from the state
            let pid: u32 = match spawned_child.get_pid().await {
                Ok(xid) => xid,
                Err(_) => {
                    let error_item = ErrorArrayItem::new(
                        Errors::InputOutput,
                        "No pid for supervised child".to_owned(),
                    );
                    log_error(state, error_item, &state_path).await;
                    wind_down_state(state, &state_path).await;
                    std::process::exit(100);
                }
            };

            // save the pid somewhere
            let pid_file: PathType =
                PathType::Content(format!("/tmp/.{}_pg.pid", state.config.app_name));

            if let Err(error) = fs::write(pid_file, pid.to_string()) {
                let error_ref = error.get_ref().unwrap_or_else(|| {
                    log!(LogLevel::Trace, "{:?}", error);
                    std::process::exit(100);
                });

                let error_item = ErrorArrayItem::new(Errors::InputOutput, error_ref.to_string());
                log_error(&mut state, error_item, &state_path).await;
                wind_down_state(&mut state, &state_path).await;
                std::process::exit(100);
            }
            log!(LogLevel::Info, "Child process spawned, pid info saved");

            if let Ok(metrics) = spawned_child.get_metrics().await {
                update_state(&mut state, &state_path, Some(metrics)).await;
            }
            return spawned_child;
        }
        Err(error) => {
            log_error(&mut state, error, &state_path).await;
            wind_down_state(&mut state, &state_path).await;
            std::process::exit(100);
        }
    }
}

pub async fn run_one_shot_process(
    settings: &AppSpecificConfig,
    state: &mut AppState,
    state_path: &PathType,
) -> Result<(), ErrorArrayItem> {
    let build_cmd = match &settings.build_command {
        Some(cmd) => cmd,
        None => {
            log!(
                LogLevel::Info,
                "No build command specified, skipping build step"
            );
            return Ok(());
        }
    };

    let parts = split(build_cmd).unwrap_or_else(|_| {
        build_cmd
            .split_whitespace()
            .map(|s| s.to_string())
            .collect()
    });
    let mut iter = parts.into_iter();
    let program = match iter.next() {
        Some(p) => p,
        None => {
            log!(LogLevel::Warn, "Exting build pre-maturly");
            return Ok(());
        }
    };

    let mut command = Command::new(program);
    for arg in iter {
        command.arg(arg);
    }

    let mut process = spawn_simple_process(&mut command, true, state, state_path)
        .await
        .map_err(ErrorArrayItem::from)?;

    if let Some(std) = process.stdout.take() {
        let buffer = BufReader::new(std);
        let mut lines = buffer.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            state.stdout.push((current_timestamp(), line));
        }
    } else {
        log!(LogLevel::Error, "Failed to capture stddout for npm install");
    }

    if let Some(std) = process.stderr.take() {
        let buffer = BufReader::new(std);
        let mut lines = buffer.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            state.stderr.push((current_timestamp(), line));
        }
    } else {
        log!(LogLevel::Error, "Failed to capture stddout for npm install");
    }

    match process.wait().await {
        Ok(status) => {
            if status.success() {
                log!(LogLevel::Debug, "build exited as expected");
                Ok(())
            } else {
                Err(ErrorArrayItem::new(
                    Errors::GeneralError,
                    format!("Build command exited with status: {}", status),
                ))
            }
        }
        Err(err) => Err(ErrorArrayItem::new(Errors::GeneralError, err.to_string())),
    }
}

// Sometimes we need a lil npm install
pub async fn run_install_process(
    settings: &AppSpecificConfig,
    state: &mut AppState,
    state_path: &PathType,
) -> Result<(), ErrorArrayItem> {
    let install_cmd = match &settings.install_command {
        Some(cmd) => cmd,
        None => {
            log!(
                LogLevel::Info,
                "No install command specified, skipping install step"
            );
            return Ok(());
        }
    };

    let parts = split(install_cmd).unwrap_or_else(|_| {
        install_cmd
            .split_whitespace()
            .map(|s| s.to_string())
            .collect()
    });
    let mut iter = parts.into_iter();
    let program = match iter.next() {
        Some(p) => p,
        None => return Ok(()),
    };

    let mut command = Command::new(program);
    for arg in iter {
        command.arg(arg);
    }

    let mut process = spawn_simple_process(&mut command, true, state, state_path)
        .await
        .map_err(ErrorArrayItem::from)?;

    if let Some(std) = process.stdout.take() {
        let buffer = BufReader::new(std);
        let mut lines = buffer.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            state.stdout.push((current_timestamp(), line));
        }
    } else {
        log!(LogLevel::Error, "Failed to capture stddout for npm install");
    }

    if let Some(std) = process.stderr.take() {
        let buffer = BufReader::new(std);
        let mut lines = buffer.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            state.stderr.push((current_timestamp(), line));
        }
    } else {
        log!(LogLevel::Error, "Failed to capture stddout for npm install");
    }

    match process.wait().await {
        Ok(status) => {
            if status.success() {
                Ok(())
            } else {
                Err(ErrorArrayItem::new(
                    Errors::GeneralError,
                    format!("Install command exited with status: {}", status),
                ))
            }
        }
        Err(err) => Err(ErrorArrayItem::new(Errors::GeneralError, err.to_string())),
    }
}
