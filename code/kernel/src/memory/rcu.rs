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
    rcu_test();
}

fn rcu_handle(v: RcuDrop) {
    local::hart_local().local_rcu.push(v);
}

fn rcu_test() {
    use alloc::boxed::Box;
    use core::sync::atomic::*;
    struct RcuSet(*const AtomicUsize, usize);
    impl Drop for RcuSet {
        fn drop(&mut self) {
            unsafe { (*self.0).store(self.1, Ordering::Relaxed) };
        }
    }
    println!("[FTL OS]rcu_test begin");
    let tm = RcuManager::<SpinNoIrq>::new();
    let v = AtomicUsize::new(0);
    let check = |a| {
        let v = v.load(Ordering::Relaxed);
        (v == a).then_some(()).ok_or((v, a))
    };
    let push = |a| tm.rcu_drop(Box::new(RcuSet(&v, a)));
    let fence = |id| {
        tm.critical_end(id, &mut Vec::new());
        tm.critical_start(id);
    };
    fence(0);
    push(1);
    check(0).unwrap();
    fence(0); // release () 1 -> current
    fence(0); // release (1)
    check(1).unwrap();
    fence(1); // release ()
    push(2);
    fence(0); // release () 2 -> current
    fence(0); // wait 1
    check(1).unwrap();
    fence(1); // release (2)
    check(2).unwrap();
    push(3);
    fence(1); // wait 0
    check(2).unwrap();
    fence(0); // release () 3 -> current
    check(2).unwrap();
    fence(1); // release (3)
    check(3).unwrap();

    println!("[FTL OS]rcu_test pass");
}
