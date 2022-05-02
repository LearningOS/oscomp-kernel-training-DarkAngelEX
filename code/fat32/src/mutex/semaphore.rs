use ftl_util::sync::{self, Spin};
pub type Semaphore = sync::semaphore::Semaphore<Spin>;
pub type MultiplySemaphore = sync::semaphore::MultiplySemaphore<Spin>;
pub type SemaphoreGuard = sync::semaphore::SemaphoreGuard<Spin>;
