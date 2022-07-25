use alloc::vec::Vec;
use ftl_util::rcu::{manager::RcuManager, RcuDrop};

use crate::{local, sync::SpinNoIrq};

const NODE_PER_LIST: usize = 300; // 每个CPU的缓存中位数

static GLOBAL_RCU_MANAGER: RcuManager<SpinNoIrq> = RcuManager::new();

/// 为了提高效率, `LocalRcuManager`并不是每次进出用户态都向全局RCU控制器提交,
/// 而是等待时钟中断到达后再提交, 彻底删除锁的使用.
///
/// 但时钟中断并不会在我们预期的时刻发生. 因此需要等待临界区结束时再关闭临界区
pub struct LocalRcuManager {
    pending: Vec<RcuDrop>,
    critical: bool,
    id: usize,
    tick: bool,
}

impl LocalRcuManager {
    pub const fn new() -> Self {
        Self {
            pending: Vec::new(),
            critical: false,
            id: usize::MAX,
            tick: false,
        }
    }
    pub fn init_id(&mut self, id: usize) {
        self.id = id;
    }
    pub fn tick(&mut self) {
        self.tick = true;
    }
    pub fn critical_start(&mut self) {
        stack_trace!();
        if self.critical {
            return;
        }
        debug_assert!(self.pending.is_empty());
        self.critical = true;
        self.tick = false;
        GLOBAL_RCU_MANAGER.critical_start(self.id)
    }
    pub fn critical_end(&mut self) {
        stack_trace!();
        if !self.critical {
            return;
        }
        self.critical = false;
        GLOBAL_RCU_MANAGER.critical_end(self.id, &mut self.pending);
    }
    pub fn critical_end_tick(&mut self) {
        if !self.tick {
            return;
        }
        self.tick = false;
        self.critical_end();
    }
    pub fn push(&mut self, v: RcuDrop) {
        debug_assert!(self.critical);
        self.pending.push(v);
    }
}

pub fn init() {
    println!("[FTL OS]rcu init");
    ftl_util::rcu::init(rcu_handle);
}

fn rcu_handle(v: RcuDrop) {
    local::hart_local().local_rcu.push(v);
}
