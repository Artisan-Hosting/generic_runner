use artisan_middleware::{
    dusa_collection_utils::{
        core::{logger::LogLevel, types::pathtype::PathType},
        log,
    }, encryption::simple_encrypt, enviornment::definitions::{Enviornment, Enviornment_V1, VERSION_TAG_V1}
};
use once_cell::sync::Lazy;
use serde_json;
use std::{fmt, string::FromUtf8Error, sync::Arc};
use tokio::{fs, process::Command, sync::RwLock};

/// Errors that can occur while loading the environment file.
#[allow(dead_code)]
#[derive(Debug)]
pub enum EnvironmentLoadError {
    Io(std::io::Error),
    Utf8(FromUtf8Error),
    InvalidFormat(String),
}

impl fmt::Display for EnvironmentLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EnvironmentLoadError::Io(err) => write!(f, "I/O error: {}", err),
            EnvironmentLoadError::Utf8(err) => write!(f, "UTF-8 conversion error: {}", err),
            EnvironmentLoadError::InvalidFormat(msg) => write!(f, "Invalid format: {}", msg),
        }
    }
}

impl std::error::Error for EnvironmentLoadError {}

impl From<std::io::Error> for EnvironmentLoadError {
    fn from(value: std::io::Error) -> Self {
        EnvironmentLoadError::Io(value)
    }
}

impl From<FromUtf8Error> for EnvironmentLoadError {
    fn from(value: FromUtf8Error) -> Self {
        EnvironmentLoadError::Utf8(value)
    }
}

pub static GLOBAL_ENVIRONMENT: Lazy<Arc<RwLock<Option<Enviornment>>>> =
    Lazy::new(|| Arc::new(RwLock::new(None)));

/// Store environment data in the global cache.
#[allow(dead_code)]
pub async fn set_global_environment(data: Enviornment) {
    let mut guard = GLOBAL_ENVIRONMENT.write().await;
    *guard = Some(data);
}

/// Retrieve environment data from the global cache.
pub async fn get_global_environment() -> Option<Enviornment> {
    GLOBAL_ENVIRONMENT.read().await.clone()
}

/// Attempt to parse an `Enviornment_V1` file.
///
/// The parser first attempts to leverage the built-in `Enviornment::parse` logic,
/// which expects encrypted bytes. If that fails, it falls back to parsing plaintext
/// data beginning with the `VERSION_TAG_V1` header.
#[allow(dead_code)]
pub async fn parse_environment_file(path: &PathType) -> Result<Enviornment, EnvironmentLoadError> {
    log!(LogLevel::Trace, "Reading environment file from {}", path);
    let bytes = fs::read(path.to_path_buf())
        .await
        .map_err(EnvironmentLoadError::Io)?;
    log!(
        LogLevel::Trace,
        "Environment file read successfully ({} bytes)",
        bytes.len()
    );

    let stupidity = simple_encrypt(&bytes).unwrap();
    print!("{}", stupidity);

    match Enviornment::parse(&bytes).await {
        Ok(environment) => {
            log!(
                LogLevel::Info,
                "Parsed Enviornment file via encrypted parser"
            );
            return Ok(environment);
        }
        Err(err) => {
            log!(
                LogLevel::Trace,
                "Encrypted parser failed for environment file: {}",
                err
            );
        }
    }

    log!(
        LogLevel::Trace,
        "Falling back to plaintext Enviornment_V1 parser for {}",
        path
    );
    let content = String::from_utf8(bytes)?;
    let mut lines = content.lines();
    let header = match lines.next() {
        Some(line) if !line.trim().is_empty() => line.trim(),
        _ => {
            return Err(EnvironmentLoadError::InvalidFormat(
                "Environment file missing version header".to_string(),
            ));
        }
    };

    if header != VERSION_TAG_V1 {
        return Err(EnvironmentLoadError::InvalidFormat(format!(
            "Unsupported environment version: {}",
            header
        )));
    }

    let json_body = lines.collect::<Vec<_>>().join("\n");
    let env_v1: Enviornment_V1 = serde_json::from_str(&json_body)
        .map_err(|err| EnvironmentLoadError::InvalidFormat(err.to_string()))?;

    log!(
        LogLevel::Info,
        "Parsed Enviornment_V1 file in plaintext mode"
    );
    Ok(Enviornment::V1(env_v1))
}

const DEFAULT_UID: u32 = 33;
const DEFAULT_GID: u32 = 33;

/// Applies settings from the cached runner environment to a [`Command`] prior to execution.
///
/// This configures UID/GID if present (falling back to a non-privileged default) and injects
/// any custom environment key/value pairs exposed by the `Enviornment_V1` schema. All failures
/// are logged but never surface as panics so command spawning remains resilient.
pub async fn apply_environment_to_command(command: &mut Command) {
    let Some(environment) = get_global_environment().await else {
        log!(
            LogLevel::Trace,
            "No runner environment cached; leaving command unchanged"
        );
        return;
    };

    match environment {
        Enviornment::V1(env) => apply_v1_environment(command, &env),
        _ => {
            log!(
                LogLevel::Warn,
                "Unsupported environment version for command overrides"
            );
        }
    }
}

fn apply_v1_environment(command: &mut Command, env: &Enviornment_V1) {
    let uid: u32 = env.execution_uid.unwrap_or(DEFAULT_UID as u16) as u32;
    command.uid(uid);
    log!(LogLevel::Trace, "Applied execution UID {} to command", uid);

    let gid: u32 = env.execution_gid.unwrap_or(DEFAULT_GID as u16) as u32;
    command.gid(gid);
    log!(LogLevel::Trace, "Applied execution GID {} to command", gid);

    if let Some((key, value)) = env.env_key_0.clone() {
        command.env(key.to_string(), value.to_string());
        log!(
            LogLevel::Info,
            "Injected custom environment variable {} for command",
            key
        );
    }

    if let Some(modifier) = env.path_modifier.as_ref() {
        let modifier_str = modifier.to_string();
        let merged_path = match std::env::var("PATH") {
            Ok(existing) if !existing.is_empty() => format!("{}:{}", modifier_str, existing),
            _ => modifier_str.clone(),
        };
        command.env("PATH", merged_path);
        log!(
            LogLevel::Info,
            "Updated PATH for command using environment modifier"
        );
    }
}
