//! Signal handling utilities.
//!
//! Separate threads listen for specific Unix signals and update shared flags
//! which the main loop can react to.

use artisan_middleware::dusa_collection_utils;
use dusa_collection_utils::core::logger::LogLevel;
use dusa_collection_utils::log;
use nix::libc::SIGUSR1;
use signal_hook::{consts::signal::SIGHUP, iterator::Signals};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;

/// Spawn a thread that listens for `SIGHUP` and toggles the provided flag.
pub fn sighup_watch(reload: Arc<AtomicBool>) {
    thread::spawn(move || {
        let mut signals = Signals::new(&[SIGHUP]).expect("Failed to register signals");
        for _ in signals.forever() {
            reload.store(true, Ordering::Relaxed);
            log!(LogLevel::Info, "Received SIGHUP, marked for reload");
        }
    });
}

/// Spawn a thread that listens for `SIGUSR1` and toggles the provided flag.
pub fn sigusr_watch(reload: Arc<AtomicBool>) {
    thread::spawn(move || {
        let mut signals = Signals::new(&[SIGUSR1]).expect("Failed to register signals");
        for _ in signals.forever() {
            reload.store(true, Ordering::Relaxed);
            log!(LogLevel::Info, "Received SIGHUP, exiting");
        }
    });
}
