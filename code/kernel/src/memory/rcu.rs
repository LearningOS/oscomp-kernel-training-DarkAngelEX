use ftl_util::rcu::manager::RcuManager;

use crate::sync::SpinNoIrq;

pub struct GlobalRcuManager {
    manager: RcuManager<SpinNoIrq>,
}

pub struct LocalRcuList {}
