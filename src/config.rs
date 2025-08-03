//! Configuration handling utilities.
//!
//! Provides helpers for loading the main [`AppConfig`], reading additional
//! application specific configuration and generating the persisted
//! [`AppState`].

use artisan_middleware::{
    aggregator::Status,
    config::AppConfig,
    dusa_collection_utils::{
        self,
        core::types::stringy::Stringy,
        core::version::{SoftwareVersion, Version, VersionCode},
    },
    state_persistence::{AppState, StatePersistence, update_state},
    timestamp::current_timestamp,
    version::{aml_version, str_to_version},
};
use colored::Colorize;
use config::{Config, ConfigError, File};
use dusa_collection_utils::{
    core::logger::{LogLevel, set_log_level},
    core::types::pathtype::PathType,
    log,
};
use serde::Deserialize;
use std::fmt;

use crate::{global_child::GLOBAL_SECRET_QUERY, secrets::SecretQuery};

/// Load the base [`AppConfig`] and populate fields derived from Cargo
/// environment variables.
pub fn get_config() -> AppConfig {
    let mut config: AppConfig = match AppConfig::new() {
        Ok(loaded_data) => loaded_data,
        Err(e) => {
            log!(LogLevel::Error, "Couldn't load config: {}", e.to_string());
            std::process::exit(100)
        }
    };
    config.app_name = Stringy::from(env!("CARGO_PKG_NAME").to_string());
    config.database = None;
    config
}

/// Load the previous [`AppState`] from disk if present, otherwise create a new
/// state structure using the provided configuration.
pub async fn generate_application_state(state_path: &PathType, config: &AppConfig) -> AppState {
    match StatePersistence::load_state(&state_path).await {
        Ok(mut loaded_data) => {
            log!(LogLevel::Info, "Loaded previous state data");
            log!(LogLevel::Trace, "Previous state data: {:#?}", loaded_data);
            loaded_data.data = String::from("Initializing");
            loaded_data.config.debug_mode = config.debug_mode;
            loaded_data.config.environment = config.environment.clone();
            loaded_data.last_updated = current_timestamp();
            loaded_data.config.log_level = config.log_level;
            loaded_data.status = Status::Starting;
            loaded_data.pid = std::process::id();
            loaded_data.stared_at = current_timestamp();
            loaded_data.stdout.clear();
            loaded_data.stderr.clear();
            set_log_level(loaded_data.config.log_level);
            loaded_data.error_log.clear();
            update_state(&mut loaded_data, &state_path, None).await;

            {
                // creating query
                let query: SecretQuery = SecretQuery::new(
                    config.app_name.to_string().replace("ais_", ""),
                    config.environment.clone(),
                    None,
                );
                _ = GLOBAL_SECRET_QUERY.set(query);
            }

            loaded_data
        }
        Err(e) => {
            log!(LogLevel::Warn, "No previous state loaded, creating new one");
            log!(LogLevel::Debug, "Error loading previous state: {}", e);
            let mut state = AppState {
                data: String::new(),
                stared_at: current_timestamp(),
                last_updated: current_timestamp(),
                event_counter: 0,
                error_log: vec![],
                config: config.clone(),
                name: config.app_name.to_string(),
                pid: std::process::id(),
                // stdout: Vec::new(),
                version: {
                    // defining the version
                    let library_version: Version = aml_version();
                    let software_version: Version =
                        str_to_version(env!("CARGO_PKG_VERSION"), Some(VersionCode::Production));

                    SoftwareVersion {
                        application: software_version,
                        library: library_version,
                    }
                },
                system_application: false,
                status: Status::Starting,
                stdout: Vec::new(),
                stderr: Vec::new(),
            };
            state.data = String::from("Initializing");
            state.config.debug_mode = config.debug_mode;
            state.last_updated = current_timestamp();
            state.config.log_level = config.log_level;
            set_log_level(state.config.log_level);
            state.error_log.clear();
            update_state(&mut state, &state_path, None).await;

            {
                // creating query
                let query: SecretQuery = SecretQuery::new(
                    config.app_name.to_string().replace("ais_", ""),
                    config.environment.clone(),
                    None,
                );
                _ = GLOBAL_SECRET_QUERY.set(query);
            }

            state
        }
    }
}

/// Read additional application specific configuration from `Config.toml`.
pub fn specific_config() -> Result<AppSpecificConfig, ConfigError> {
    let mut builder = Config::builder();
    builder = builder.add_source(File::with_name("Config").required(false));

    let settings = builder.build()?;
    let app_specific: AppSpecificConfig = settings.get("app_specific")?;

    Ok(app_specific)
}

/// Configuration section located under `[app_specific]` in `Config.toml`.
#[derive(Debug, Deserialize, Clone)]
pub struct AppSpecificConfig {
    pub interval_seconds: u32,
    pub monitor_path: String,
    pub project_path: String,
    pub changes_needed: i32,
    pub ignored_subdirs: Vec<String>, // Add ignored subdirectories as strings
    #[serde(default)]
    pub install_command: Option<String>,
    #[serde(default)]
    pub build_command: Option<String>,
    pub run_command: String,
    pub secret_server_addr: String,
    pub env_file_location: String,
}

#[allow(dead_code)]
impl AppSpecificConfig {
    pub fn safe_path(&self) -> PathType {
        let self_cloned = self.clone();
        let path = PathType::Content(self_cloned.monitor_path);
        if !path.exists() {
            log!(LogLevel::Error, "The path {} doesn't exist", path);
            std::process::exit(0)
        } else {
            match path.canonicalize() {
                Ok(canon_path) => PathType::PathBuf(canon_path),
                Err(e) => {
                    log!(
                        LogLevel::Error,
                        "Failed to canonicalize path: {}, using default: {}",
                        e,
                        path
                    );
                    path
                }
            }
        }
    }

    pub fn project_path(&self) -> PathType {
        let self_cloned = self.clone();
        let path = PathType::Content(self_cloned.project_path);
        if !path.exists() {
            log!(LogLevel::Error, "The path {} doesn't exist", path);
            std::process::exit(0)
        } else {
            match path.canonicalize() {
                Ok(canon_path) => PathType::PathBuf(canon_path),
                Err(e) => {
                    log!(
                        LogLevel::Error,
                        "Failed to canonicalize path: {}, using default: {}",
                        e,
                        path
                    );
                    path
                }
            }
        }
    }

    /// Converts ignored_subdirs strings into PathType objects relative to the monitor_path
    pub fn ignored_paths(&self) -> Vec<PathType> {
        let base_path = self.safe_path(); // Canonicalize the monitor path

        let sub_dirs: Vec<PathType> = self
            .ignored_subdirs
            .iter()
            .map(|subdir| PathType::PathBuf(base_path.join(subdir))) // Join each subdir to the base path
            .collect();

        if sub_dirs.is_empty() {
            return Vec::new();
        }

        return sub_dirs;
    }
}

impl fmt::Display for AppSpecificConfig {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{} {{\n\
             \t{}: {},\n\
             \t{}: {},\n\
             \t{}: {},\n\
             \t{}: {},\n\
             \t{}: {},\n\
             \t{}: {:?},\n\
             \t{}: {:?},\n\
             \t{}: {},\n\
             }}",
            "AppSpecificConfig".cyan().bold(),
            "interval_seconds".yellow(),
            self.interval_seconds.to_string().green(),
            "monitor_path".yellow(),
            self.monitor_path.clone().green(),
            "project_path".yellow(),
            self.project_path.clone().green(),
            "changes_needed".yellow(),
            self.changes_needed.to_string().green(),
            "Ignored_directories".yellow(),
            self.ignored_subdirs.join(" ").green(),
            "install_command".yellow(),
            self.install_command,
            "build_command".yellow(),
            self.build_command,
            "run_command".yellow(),
            self.run_command.clone().green()
        )
    }
}
