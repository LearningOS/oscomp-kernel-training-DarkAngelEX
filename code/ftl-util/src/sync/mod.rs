pub mod spin_mutex;
pub mod rw_spin_mutex;
pub mod sleep_mutex;
pub mod rw_sleep_mutex;
pub mod semaphore;

/// Low-level support for mutex
pub trait MutexSupport {
    type GuardData;
    /// Called before lock() & try_lock()
    fn before_lock() -> Self::GuardData;
    /// Called when MutexGuard dropping
    fn after_unlock(_: &mut Self::GuardData);
}

/// Spin lock
#[derive(Debug)]
pub struct Spin;

impl MutexSupport for Spin {
    type GuardData = ();
    #[inline(always)]
    fn before_lock() -> Self::GuardData {}
    #[inline(always)]
    fn after_unlock(_: &mut Self::GuardData) {}
}

pub fn seq_fence() {
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
}