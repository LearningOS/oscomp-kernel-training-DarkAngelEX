#![allow(dead_code)]
use ftl_util::sync::{self, Spin};
pub type SpinMutex<T> = sync::spin_mutex::SpinMutex<T, Spin>;
