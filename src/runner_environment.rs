//! Applies structural settings (execution UID/GID, PATH modifier) from the
//! runtime config to a [`Command`] prior to spawning the actual application.
//!
//! Arbitrary custom secrets/env vars are no longer read from any file or
//! decrypted here at all -- they arrive already set in this process's own
//! environment (watchdog sets them when spawning `generic_runner` itself,
//! sourced from the runtime bundle's `.env` content), and `Command` inherits
//! the parent's environment by default unless explicitly cleared, so they
//! reach the spawned child with no code needed on this side. This replaces
//! the old `Enviornment_V1`-file-parsing + secret-server-fetch machinery
//! entirely.

use artisan_middleware::dusa_collection_utils::{core::logger::LogLevel, log};
use tokio::process::Command;

use crate::config::AppSpecificConfig;

/// Applies `settings`' execution UID/GID (if explicitly configured) and PATH
/// modifier to `command`. Deliberately does *not* default to a hardcoded
/// uid/gid when unset -- watchdog already spawns `generic_runner` itself as
/// www-data in production (`configure_client_runtime_command`), so leaving
/// these untouched here means the child simply inherits that, the same as
/// any other unset process attribute. Forcing a default here previously
/// broke `cargo test` (which runs as a regular, non-root user and can't
/// `setuid` to an arbitrary uid) for no real production benefit.
pub fn apply_environment_to_command(command: &mut Command, settings: &AppSpecificConfig) {
    if let Some(uid) = settings.execution_uid {
        command.uid(uid as u32);
        log!(LogLevel::Trace, "Applied execution UID {} to command", uid);
    }

    if let Some(gid) = settings.execution_gid {
        command.gid(gid as u32);
        log!(LogLevel::Trace, "Applied execution GID {} to command", gid);
    }

    if let Some(modifier) = settings.path_modifier.as_ref() {
        let merged_path = match std::env::var("PATH") {
            Ok(existing) if !existing.is_empty() => format!("{}:{}", modifier, existing),
            _ => modifier.clone(),
        };
        command.env("PATH", merged_path);
        log!(
            LogLevel::Info,
            "Updated PATH for command using environment modifier"
        );
    }
}
