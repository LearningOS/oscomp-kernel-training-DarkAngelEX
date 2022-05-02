use ftl_util::sync::{self, Spin};
pub type RwSpinMutex<T> = sync::rw_spin_mutex::RwSpinMutex<T, Spin>;
