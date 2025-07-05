use std::sync::Arc;
use once_cell::sync::Lazy;
use tokio::sync::Mutex;
use artisan_middleware::process_manager::SupervisedChild;

/// Globally available reference to the current [`SupervisedChild`].
/// It is wrapped in an [`Arc`] and [`Mutex`] so it can be safely
/// shared and modified across threads.
pub static GLOBAL_CHILD: Lazy<Arc<Mutex<Option<SupervisedChild>>>> = Lazy::new(|| {
    Arc::new(Mutex::new(None))
});

/// Initialize the global child value. This is typically called once
/// at start up after the first child is spawned.
pub async fn init(child: SupervisedChild) {
    let mut lock = GLOBAL_CHILD.lock().await;
    *lock = Some(child);
}

/// Replace the currently stored child with a new one. This allows
/// other threads to always access the latest child handle.
pub async fn replace(child: SupervisedChild) {
    let mut lock = GLOBAL_CHILD.lock().await;
    *lock = Some(child);
}
