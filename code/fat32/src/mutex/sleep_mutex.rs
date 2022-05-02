use ftl_util::sync::{self, Spin};
pub type SleepMutex<T> = sync::sleep_mutex::SleepMutex<T, Spin>;
