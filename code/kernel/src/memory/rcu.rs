use alloc::vec::Vec;
use ftl_util::rcu::{manager::RcuManager, RcuDrop};

use crate::{local, sync::SpinNoIrq, xdebug::CRITICAL_END_FORCE};

const NODE_PER_LIST: usize = 300; // 每个CPU的缓存中位数

static GLOBAL_RCU_MANAGER: RcuManager<SpinNoIrq> = RcuManager::new();

/// 为了提高效率, `LocalRcuManager`并不会每次进出用户态或切换线程都向
/// 全局RCU控制器提交释放队列, 而是等待时钟中断到达后再提交, 彻底删除锁竞争
///
/// 但时钟中断并不会在我们预期的时刻发生. 因此需要等待临界区结束时再关闭临界区
///
/// 此管理器允许中断时使用, FTL OS最大嵌套次数为2
pub struct LocalRcuManager {
    pending: Vec<RcuDrop>,     // 此CPU提交的释放队列
    pending_rec: Vec<RcuDrop>, // 发生嵌套时提交的队列
    id: usize,                 // CPU编号, 保证唯一性即可
    critical: bool,            // 仅用于 debug_assert
    tick: bool,                // 时钟中断到达标志
    rec: bool,                 // 嵌套标志
}

impl LocalRcuManager {
    pub const fn new() -> Self {
        Self {
            pending: Vec::new(),
            pending_rec: Vec::new(),
            id: usize::MAX,
            critical: false,
            tick: false,
            rec: false,
        }
    }
    pub fn init_id(&mut self, id: usize) {
        self.id = id;
    }
    // 时钟中断会调用这个函数
    pub fn tick(&mut self) {
        self.tick = true;
    }
    fn rec(&mut self) -> bool {
        unsafe { core::ptr::read_volatile(&self.rec) }
    }
    fn set_rec(&mut self) {
        unsafe { core::ptr::write_volatile(&mut self.rec, true) }
    }
    fn clear_rec(&mut self) {
        unsafe { core::ptr::write_volatile(&mut self.rec, false) }
    }

    // 进入RCU临界区
    #[inline]
    pub fn critical_start(&mut self) {
        stack_trace!();
        if self.critical {
            return;
        }
        self.critical = true;
        self.tick = false;
        GLOBAL_RCU_MANAGER.critical_start(self.id)
    }
    /// 强制结束RCU临界区并刷入缓存的释放队列
    ///
    /// 允许开中断, 因为 pending 只会在锁内被修改
    pub fn critical_end(&mut self) {
        stack_trace!();
        if !self.critical && self.pending.is_empty() {
            return;
        }
        self.critical = false;
        self.set_rec();
        if !self.pending_rec.is_empty() {
            self.pending.append(&mut self.pending_rec);
        }
        GLOBAL_RCU_MANAGER.critical_end(self.id, &mut self.pending);
        self.clear_rec();
    }
    /// 当时钟中断到达了才会将tick设为true, 此时才离开临界区
    #[inline]
    pub fn critical_end_tick(&mut self) {
        if !CRITICAL_END_FORCE && !self.tick {
            return;
        }
        self.tick = false;
        self.critical_end();
    }
    pub fn push(&mut self, v: RcuDrop) {
        debug_assert!(self.critical);
        self.special_push(v);
    }
    /// special_push 允许在临界区之外运行, 作用是提交未释放内存
    pub fn special_push(&mut self, v: RcuDrop) {
        if self.rec() {
            self.pending_rec.push(v);
        } else {
            self.set_rec();
            self.pending.push(v);
            self.clear_rec();
        }
    }
}

pub fn init() {
    println!("[FTL OS]rcu init");
    ftl_util::rcu::init(rcu_release);
    rcu_test();
}
/// 提交到当前CPU的RCU释放队列
pub fn rcu_release(v: RcuDrop) {
    local::hart_local().local_rcu.push(v);
}
/// 这个函数允许在RCU临界区之外向当前CPU提交RCU释放队列
pub fn rcu_special_release(v: RcuDrop) {
    local::hart_local().local_rcu.special_push(v)
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
