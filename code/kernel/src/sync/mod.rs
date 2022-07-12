use ftl_util::sync::MutexSupport;

use crate::hart::interrupt;

pub mod even_bus;
pub mod mutex;

pub type SleepMutex<T> = ftl_util::sync::sleep_mutex::SleepMutex<T, SpinNoIrq>;
pub type RwSleepMutex<T> = ftl_util::sync::rw_sleep_mutex::RwSleepMutex<T, SpinNoIrq>;

/// Spin & no-interrupt lock
#[derive(Debug)]
pub struct SpinNoIrq;

/// Contains RFLAGS before disable interrupt, will auto restore it when dropping
pub struct FlagsGuard(bool);

impl Drop for FlagsGuard {
    fn drop(&mut self) {
        unsafe { interrupt::restore(self.0) };
    }
}

impl FlagsGuard {
    pub fn no_irq_region() -> Self {
        Self(unsafe { interrupt::disable_and_store() })
    }
}

impl MutexSupport for SpinNoIrq {
    type GuardData = FlagsGuard;
    #[inline(always)]
    fn before_lock() -> Self::GuardData {
        FlagsGuard::no_irq_region()
    }
    fn after_unlock(_: &mut Self::GuardData) {}
}
