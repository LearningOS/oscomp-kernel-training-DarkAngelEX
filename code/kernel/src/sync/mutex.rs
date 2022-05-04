#![allow(dead_code)]

use ftl_util::sync::{spin_mutex::SpinMutex, Spin};

use super::SpinNoIrq;

pub type SpinLock<T> = SpinMutex<T, Spin>;
pub type SpinNoIrqLock<T> = SpinMutex<T, SpinNoIrq>;
