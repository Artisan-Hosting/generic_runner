//! Global handles to the running child process and directory monitor.
//!
//! These are wrapped in [`Arc`] and [`Mutex`] so that various tasks in the
//! application can access the latest child or monitor instance.

use artisan_middleware::process_manager::SupervisedChild;
use dir_watcher::RawFileMonitor;
use once_cell::sync::{Lazy, OnceCell};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::secrets::{SecretClient, SecretQuery};

/// Globally available reference to the current [`SupervisedChild`].
/// It is wrapped in an [`Arc`] and [`Mutex`] so it can be safely
/// shared and modified across threads.
pub static GLOBAL_CHILD: Lazy<Arc<Mutex<Option<SupervisedChild>>>> =
    Lazy::new(|| Arc::new(Mutex::new(None)));

/// Globally available reference to the current [`RawFileMonitor`].
/// It is wrapped in an [`Arc`] and [`Mutex`] so it can be safely
/// shared and modified across threads.
pub static GLOBAL_MONITOR: Lazy<Arc<Mutex<Option<RawFileMonitor>>>> =
    Lazy::new(|| Arc::new(Mutex::new(None)));

/// Globally available refrence to the current [`SecretQuery`].
pub static GLOBAL_SECRET_QUERY: OnceCell<SecretQuery> = OnceCell::new();

/// Globally available persistente connection to the secrets server
pub static GLOBAL_CLINENT_CONNECTION: Lazy<Arc<Mutex<Option<SecretClient>>>> =
    Lazy::new(|| Arc::new(Mutex::const_new(None)));

/// Initialize the global child value. This is typically called once
/// at start up after the first child is spawned.
pub async fn init_child(child: SupervisedChild) {
    let mut lock = GLOBAL_CHILD.lock().await;
    *lock = Some(child);
}

/// Replace the currently stored child with a new one. This allows
/// other threads to always access the latest child handle.
pub async fn replace_child(child: SupervisedChild) {
    let mut lock = GLOBAL_CHILD.lock().await;
    *lock = Some(child);
}

/// Initialize the global child value. This is typically called once
/// at start up after the first child is spawned.
pub async fn init_monitor(monitor: RawFileMonitor) {
    let mut lock = GLOBAL_MONITOR.lock().await;
    *lock = Some(monitor);
}

pub fn get_query() -> Result<SecretQuery, ()> {
    if let Some(query) = GLOBAL_SECRET_QUERY.get() {
        Ok(query.clone())
    } else {
        Err(())
    }
}

// 100.105.82.205
// 10.2.0.3
