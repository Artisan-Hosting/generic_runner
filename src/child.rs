//! Utilities for spawning and monitoring child processes.

use artisan_middleware::dusa_collection_utils::core::errors::Errors;
use artisan_middleware::dusa_collection_utils::core::functions::current_timestamp;
use artisan_middleware::dusa_collection_utils::log;
use artisan_middleware::process_manager::{
    SupervisedChild, spawn_complex_process, spawn_simple_process,
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
use crate::global_child::CHILD_ENV;

/// Turns the secret server's `(key, value)` pairs into environment variables.
/// A value that is not valid UTF-8 is skipped with a warning instead of taking
/// the whole runner down (this used to `unwrap()` it).
pub fn secrets_to_env(secrets: &[(String, Vec<u8>)]) -> Vec<(String, String)> {
    secrets
        .iter()
        .filter_map(|(key, value)| match std::str::from_utf8(value) {
            Ok(text) => Some((key.clone(), text.to_owned())),
            Err(_) => {
                log!(LogLevel::Warn, "Skipping secret {}: its value is not valid UTF-8", key);
                None
            }
        })
        .collect()
}

/// Builds the command for one of the app's shell-quoted command lines
/// (`install_command`, `build_command`, `run_command`), running in the app's
/// own directory with the app's environment.
///
/// All three used to differ: only the run command had a working directory, so
/// `npm install` and the build ran in the runner's config directory, not the
/// checkout. Returns `None` for an empty command line.
pub fn command_for(line: &str, cwd: &str, env: &[(String, String)]) -> Option<Command> {
    let parts = split(line).unwrap_or_else(|_| line.split_whitespace().map(|s| s.to_string()).collect());
    let mut iter = parts.into_iter();
    let program = iter.next()?;
    let mut command = Command::new(program);
    for arg in iter {
        command.arg(arg);
    }
    command.current_dir(cwd);
    for (key, value) in env {
        command.env(key, value);
    }
    Some(command)
}

/// The environment for the app's commands: the fetched secrets. (`PORT` is set by
/// the watchdog on the runner itself and inherited, so it is not repeated here.)
fn app_env() -> Vec<(String, String)> {
    CHILD_ENV.get().cloned().unwrap_or_default()
}

/// Spawn the main child process defined in [`AppSpecificConfig`].
///
/// The spawned process is wrapped in [`SupervisedChild`] so that
/// stdout/stderr and metrics can be monitored.
pub async fn create_child(
    mut state: &mut AppState,
    state_path: &PathType,
    settings: &AppSpecificConfig,
) -> SupervisedChild {
    log!(LogLevel::Trace, "Creating child process...");

    let project_dir = settings.project_path();
    let mut command: Command = match command_for(&settings.run_command, &project_dir.to_string(), &app_env()) {
        Some(command) => command,
        None => {
            let error_item = ErrorArrayItem::new(Errors::InputOutput, "run_command is empty".to_owned());
            log_error(state, error_item, &state_path).await;
            wind_down_state(state, &state_path).await;
            std::process::exit(100);
        }
    };

    match spawn_complex_process(&mut command, Some(project_dir), false, true).await {
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

/// Execute the optional build command defined in the configuration.
///
/// Any output produced by the process is stored in the [`AppState`] buffers.
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

    let mut command = match command_for(build_cmd, &settings.project_path().to_string(), &app_env()) {
        Some(command) => command,
        None => {
            log!(LogLevel::Warn, "Exting build pre-maturly");
            return Ok(());
        }
    };

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

/// Optionally run an install command before building the project.
///
/// This is useful for fetching dependencies such as `npm install` prior to
/// spawning the main child process.
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

    let mut command = match command_for(install_cmd, &settings.project_path().to_string(), &app_env()) {
        Some(command) => command,
        None => return Ok(()),
    };

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

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of(command: &Command) -> Vec<(String, Option<String>)> {
        command
            .as_std()
            .get_envs()
            .map(|(k, v)| (k.to_string_lossy().into_owned(), v.map(|v| v.to_string_lossy().into_owned())))
            .collect()
    }

    #[test]
    fn a_command_runs_in_the_apps_directory_with_its_arguments_and_environment() {
        let env = vec![("DB_URL".to_owned(), "mysql://x".to_owned()), ("PORT".to_owned(), "20001".to_owned())];
        let command = command_for("npm run \"build all\" --silent", "/var/www/ais/abc12345", &env).unwrap();
        let std = command.as_std();
        assert_eq!(std.get_program(), "npm");
        assert_eq!(
            std.get_args().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>(),
            ["run", "build all", "--silent"],
            "quoted arguments stay one argument"
        );
        assert_eq!(std.get_current_dir().unwrap().to_string_lossy(), "/var/www/ais/abc12345");
        let vars = env_of(&command);
        assert!(vars.contains(&("DB_URL".to_owned(), Some("mysql://x".to_owned()))));
        assert!(vars.contains(&("PORT".to_owned(), Some("20001".to_owned()))));
    }

    #[test]
    fn an_empty_command_line_is_no_command() {
        assert!(command_for("   ", "/tmp", &[]).is_none());
    }

    #[test]
    fn secrets_become_environment_variables_and_bad_values_are_skipped_not_fatal() {
        let secrets = vec![
            ("API_KEY".to_owned(), b"abc".to_vec()),
            ("BROKEN".to_owned(), vec![0xff, 0xfe]),
            ("EMPTY".to_owned(), Vec::new()),
        ];
        let env = secrets_to_env(&secrets);
        assert_eq!(env, vec![("API_KEY".to_owned(), "abc".to_owned()), ("EMPTY".to_owned(), String::new())]);
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CommandKind {
    Install,
    Build,
    Run,
}

pub fn command_for_mode(
    _kind: CommandKind,
    _line: &str,
    _cwd: &str,
    _env: &[(String, String)],
    _shell: bool,
) -> Option<Command> {
    todo!()
}
