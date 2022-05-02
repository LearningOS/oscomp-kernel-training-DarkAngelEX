use ftl_util::sync::{self, Spin};
pub type RwSleepMutex<T> = sync::rw_sleep_mutex::RwSleepMutex<T, Spin>;
pub type RwSpinMutex<T> = sync::rw_spin_mutex::RwSpinMutex<T, Spin>;
pub type Semaphore = sync::semaphore::Semaphore<Spin>;
pub type MultiplySemaphore = sync::semaphore::MultiplySemaphore<Spin>;
pub type SemaphoreGuard = sync::semaphore::SemaphoreGuard<Spin>;
pub type SleepMutex<T> = sync::sleep_mutex::SleepMutex<T, Spin>;
pub type SpinMutex<T> = sync::spin_mutex::SpinMutex<T, Spin>;
