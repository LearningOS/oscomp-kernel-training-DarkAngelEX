use ftl_util::sync::{self, Spin};
pub type RwSleepMutex<T> = sync::rw_sleep_mutex::RwSleepMutex<T, Spin>;
