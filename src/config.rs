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
    enviornment::definitions::Enviornment_V2,
    state_persistence::{AppState, StatePersistence, update_state},
    timestamp::current_timestamp,
    version::{aml_version, str_to_version},
};
use colored::Colorize;
use dusa_collection_utils::{
    core::logger::{LogLevel, set_log_level},
    core::types::pathtype::PathType,
    log,
};
use serde::Deserialize;
use std::fmt;

/// Reads the runtime bundle's unpacked fixed config (`runtime.toml`,
/// written by watchdog into this app's config directory -- our cwd is
/// already set there by whatever spawned us) into `Enviornment_V2`.
///
/// Replaces the old two-file `AppConfig` (from `Overrides.toml`) /
/// `AppSpecificConfig` (from `Config.toml`'s `[app_specific]`) load: both
/// are now projections of this one struct (see `get_config`/
/// `specific_config` below) instead of two independently-loaded files, and
/// there's no encryption step here -- watchdog already decrypted the bundle
/// before writing this file out.
fn load_runtime_config() -> Enviornment_V2 {
    let content = match std::fs::read_to_string("runtime.toml") {
        Ok(content) => content,
        Err(e) => {
            log!(LogLevel::Error, "Couldn't read runtime.toml: {}", e);
            std::process::exit(100)
        }
    };
    match toml::from_str(&content) {
        Ok(config) => config,
        Err(e) => {
            log!(LogLevel::Error, "Couldn't parse runtime.toml: {}", e);
            std::process::exit(100)
        }
    }
}

/// Load the base [`AppConfig`] view of the runtime config, with fields
/// derived from Cargo environment variables applied the same way the old
/// `Overrides.toml`-backed loader did.
pub fn get_config() -> AppConfig {
    let fixed = load_runtime_config();
    AppConfig {
        app_name: Stringy::from(env!("CARGO_PKG_NAME").to_string()),
        max_ram_usage: fixed.max_ram_usage,
        max_cpu_usage: fixed.max_cpu_usage,
        environment: fixed.environment.to_string(),
        debug_mode: fixed.debug_mode,
        log_level: fixed.log_level,
        git: fixed.git,
        // Never meaningfully used by this runner; matches the old loader's
        // own `config.database = None;` override.
        database: None,
        aggregator: fixed.aggregator,
    }
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

            state
        }
    }
}

/// Read the `[app_specific]`-shaped view of the runtime config, plus the
/// execution uid/gid/PATH-modifier fields `apply_environment_to_command`
/// needs -- folded in here (rather than a third projection type) so
/// `create_child` and friends don't need a second config parameter.
pub fn specific_config() -> Result<AppSpecificConfig, String> {
    let fixed = load_runtime_config();
    Ok(AppSpecificConfig {
        interval_seconds: fixed.interval_seconds,
        monitor_path: fixed.monitor_path.to_string(),
        project_path: fixed.project_path.to_string(),
        changes_needed: fixed.changes_needed,
        ignored_subdirs: fixed.ignored_subdirs.iter().map(|s| s.to_string()).collect(),
        install_command: fixed.install_command.map(|s| s.to_string()),
        build_command: fixed.build_command.map(|s| s.to_string()),
        run_command: fixed.run_command.to_string(),
        execution_uid: fixed.execution_uid,
        execution_gid: fixed.execution_gid,
        path_modifier: fixed.path_modifier.map(|s| s.to_string()),
    })
}

/// Runner execution settings, projected from `Enviornment_V2` (see
/// `specific_config`) -- no longer read from `Config.toml`'s `[app_specific]`
/// table directly.
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
    #[serde(default)]
    pub execution_uid: Option<u16>,
    #[serde(default)]
    pub execution_gid: Option<u16>,
    #[serde(default)]
    pub path_modifier: Option<String>,
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
