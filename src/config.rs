//! Configuration handling utilities.
//!
//! Provides helpers for loading the main [`AppConfig`], reading additional
//! application specific configuration and generating the persisted
//! [`AppState`].

use artisan_middleware::{
    aggregator::Status,
    config::AppConfig,
    custom_config::CustomConfig,
    dusa_collection_utils::{
        self,
        core::types::stringy::Stringy,
        core::version::{SoftwareVersion, Version, VersionCode},
    },
    enviornment::definitions::Enviornment_V2,
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

/// Attempts to load configuration from Environment V2 control-plane files
/// (`runtime.toml` and optional `custom.json`) unpacked by watchdog.
pub fn load_runtime_v2() -> Option<(AppConfig, AppSpecificConfig)> {
    let runtime_path = std::path::Path::new("runtime.toml");
    if !runtime_path.exists() {
        return None;
    }

    let content = match std::fs::read_to_string(runtime_path) {
        Ok(c) => c,
        Err(err) => {
            log!(LogLevel::Warn, "Failed to read runtime.toml: {}", err);
            return None;
        }
    };

    let env_v2: Enviornment_V2 = match toml::from_str(&content) {
        Ok(env) => env,
        Err(err) => {
            log!(LogLevel::Warn, "Failed to parse runtime.toml as Enviornment_V2: {}", err);
            return None;
        }
    };

    let app_config = AppConfig {
        app_name: env_v2.app_name.clone(),
        max_ram_usage: env_v2.max_ram_usage,
        max_cpu_usage: env_v2.max_cpu_usage,
        environment: env_v2.environment.to_string(),
        debug_mode: env_v2.debug_mode,
        log_level: env_v2.log_level,
        git: env_v2.git.clone(),
        database: env_v2.database.clone(),
        aggregator: env_v2.aggregator.clone(),
    };

    let mut secret_server_addr = default_secret_server();
    let mut env_file_location = default_env_location();

    let custom_path = std::path::Path::new("custom.json");
    if custom_path.exists() {
        if let Ok(custom_str) = std::fs::read_to_string(custom_path) {
            if let Ok(custom) = CustomConfig::from_json(&custom_str) {
                if let Some(addr) = custom.get::<String>("secret_server_addr") {
                    secret_server_addr = addr;
                }
                if let Some(loc) = custom.get::<String>("env_file_location") {
                    env_file_location = loc;
                }
            }
        }
    }

    let specific_config = AppSpecificConfig {
        interval_seconds: env_v2.interval_seconds,
        monitor_path: env_v2.monitor_path.to_string(),
        project_path: env_v2.project_path.to_string(),
        changes_needed: env_v2.changes_needed,
        ignored_subdirs: env_v2.ignored_subdirs.iter().map(|s| s.to_string()).collect(),
        install_command: env_v2.install_command.as_ref().map(|s| s.to_string()),
        build_command: env_v2.build_command.as_ref().map(|s| s.to_string()),
        run_command: env_v2.run_command.to_string(),
        secret_server_addr,
        env_file_location,
    };

    Some((app_config, specific_config))
}

/// Load the base [`AppConfig`] and populate fields derived from Cargo
/// environment variables, checking `runtime.toml` first.
pub fn get_config() -> AppConfig {
    if let Some((app_config, _)) = load_runtime_v2() {
        log!(LogLevel::Info, "Loaded base configuration from Environment V2 (runtime.toml)");
        return app_config;
    }

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

/// Read additional application specific configuration, checking `runtime.toml`/`custom.json`
/// first before falling back to `Config.toml`.
pub fn specific_config() -> Result<AppSpecificConfig, ConfigError> {
    if let Some((_, specific_config)) = load_runtime_v2() {
        log!(LogLevel::Info, "Loaded app-specific configuration from Environment V2 (runtime.toml)");
        return Ok(specific_config);
    }

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
    #[serde(default = "default_secret_server")]
    pub secret_server_addr: String,
    #[serde(default = "default_env_location")]
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

pub fn default_secret_server() -> String { String::from("localhost:50051") }
pub fn default_env_location() -> String { String::from("/tmp/.trash") }