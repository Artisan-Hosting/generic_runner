use artisan_middleware::{common::{log_error, update_state}, log, logger::LogLevel, state_persistence::AppState};
use dusa_collection_utils::{errors::ErrorArrayItem, types::PathType};
use nix::libc;
use std::process::Stdio;
use tokio::process::{Child, Command};

use crate::config::{wind_down_state, AppSpecificConfig};

pub async fn create_child(
    state: &mut AppState,
    state_path: &PathType,
    settings: &AppSpecificConfig,
) -> Child {
    log!(LogLevel::Trace, "Creating child process...");

    let mut command = Command::new("npm");
    command
        .args(&["--prefix", &settings.clone().project_path, "run", "start"]) // Updated to run "build" instead of "start"
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("NODE_ENV", "production") // Set NODE_ENV=production
        .env("PORT", "3000"); // Set PORT=3000

    // Set the process to start a new process group
    unsafe {
        command.pre_exec(|| {
            // Set the child process's group ID to its own PID
            libc::setpgid(0, 0);
            Ok(())
        });
    }

    let child = match command.spawn() {
        Ok(loaded_child) => {
            log!(
                LogLevel::Trace,
                "Child process spawned successfully: {:#?}",
                loaded_child
            );
            state.data = String::from("Application spawned");
            state.event_counter += 1;
            update_state(state, state_path);
            loaded_child
        }
        Err(e) => {
            log!(LogLevel::Error, "Failed to spawn child process: {}", e);
            log_error(state, ErrorArrayItem::from(e), state_path);
            wind_down_state(state, state_path);
            std::process::exit(0)
        }
    };
    child
}

pub async fn run_one_shot_process(settings: &AppSpecificConfig) -> Result<(), String> {
    // Set the environment variable NODE_ENV to "production"
    let output = Command::new("npm")
        .arg("--prefix")
        .arg(settings.clone().project_path)
        .arg("run")
        .arg("build")
        .env("NODE_ENV", "production") // Add this line to set NODE_ENV=production
        .output()
        .await
        .map_err(|err| format!("Failed to execute npm run build: {}", err))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    log!(LogLevel::Debug, "Standard Out: {}", stdout);
    log!(LogLevel::Debug, "Standard Err: {}", stderr);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("npm run build failed: {}", stderr));
    }

    Ok(())
}
