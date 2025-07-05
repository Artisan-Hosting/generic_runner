use artisan_middleware::process_manager::SupervisedChild;
use dir_watcher::object::{MonitorMode, RawFileMonitor};
use dir_watcher::options::Options;
use once_cell::sync::Lazy;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::broadcast::{Receiver};
use notify::{Event, RecursiveMode};

// pub static GLOBAL_OPTIONS: Lazy<Arc<Options>> = Lazy::new(|| Arc::new(Options::default()
//         .set_mode(RecursiveMode::Recursive)
//         .set_monitor_mode(MonitorMode::Modify)
//         // .add_ignored_dirs(settings.ignored_paths())
//         // .set_target_dir(settings.safe_path())
//         // .set_interval(settings.interval_seconds.into())
//         .set_validation(true)));

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

/// Replace the currently stored child with a new one. This allows
/// other threads to always access the latest child handle.
pub async fn replace_monitor(monitor: RawFileMonitor) {
    let mut lock = GLOBAL_MONITOR.lock().await;
    *lock = Some(monitor);
}

pub async fn get_event_reciver() -> Result<Receiver<Event>, ()> {
    if let Some(monitor) = GLOBAL_MONITOR.lock().await.as_mut() {
        monitor.health_check(false).await;
        if let Some(recv) = monitor.subscribe().await {
            return Ok(recv);
        }
    }
    Err(())
}
