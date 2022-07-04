use core::sync::atomic::{AtomicU64, Ordering};

use alloc::vec::Vec;

use crate::sync::{spin_mutex::SpinMutex, MutexSupport};

use super::{RcuCollect, RcuDrop};

/// RCU 回收系统
///
/// flags: [离开临界区的核心|启动的临界区的核心]
///
/// 运行方式:
///
/// 每个核心必须绑定至一个ID, 每个ID都可以发起一段临界区,
/// 临界区内RCU保护的变量保证不会调用析构函数
///
/// 管理器包含两个集合, current / pending. current 只能从 pending 转换而来, 不允许追加.
/// 当所有核心的临界区结束时将释放current中的全部元素
///
/// flags划分为2部分, [6:32|31:0] = [current|pending]
///     current: rcu_current 临界区运行的核心
///     pending: 正在临界区运行的核心
///
/// 发起临界区:
/// flags_pending 设置CPU对应位 表示 rcu_pending 包含本CPU的临界区
///
/// 离开临界区:
/// flags_current 删除对应CPU位并判断, 如果为0则:
///     转移 flags_pending 至 flags_current, 增加当前CPU位防止顺序错误
///     释放 rcu_current, 转移 rcu_pending 至 rcu_current
///     删除当前CPU位, 这期间收集到的内存留给下个释放周期释放
pub struct RcuManager<S: MutexSupport> {
    flags: AtomicU64,
    rcu_current: SpinMutex<Vec<RcuDrop>, S>,
    rcu_pending: SpinMutex<Vec<RcuDrop>, S>,
}

impl<S: MutexSupport> RcuManager<S> {
    pub const fn new() -> Self {
        Self {
            flags: AtomicU64::new(0),
            rcu_current: SpinMutex::new(Vec::new()),
            rcu_pending: SpinMutex::new(Vec::new()),
        }
    }
    pub fn critical_start(&self, id: usize) {
        debug_assert!(id < 32);
        let mask_pending = 1 << id;
        if self.flags.load(Ordering::Relaxed) & mask_pending == 0 {
            self.flags.fetch_or(mask_pending, Ordering::Acquire);
        }
    }
    pub fn critical_end(&self, id: usize) {
        debug_assert!(id < 32);
        let mask_pending = 1 << id;
        let mask_current = mask_pending << 32;
        let mask_all = mask_pending | mask_current;
        let mut release;
        let mut need_release;
        let mut prev = self.flags.load(Ordering::Relaxed);
        if prev & mask_all == 0 {
            return;
        }
        loop {
            let mut next = prev & !mask_all;
            release = (next >> 32) == 0;
            need_release = unsafe {
                !self.rcu_pending.unsafe_get().is_empty()
                    || !self.rcu_current.unsafe_get().is_empty()
            };
            if release {
                next <<= 32;
                if need_release {
                    next |= mask_current;
                }
            }
            match self
                .flags
                .compare_exchange(prev, next, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => break,
                Err(v) => prev = v,
            }
        }
        if !release || !need_release {
            return;
        }
        let pending = core::mem::take(&mut *self.rcu_pending.lock());
        let v = core::mem::replace(&mut *self.rcu_current.lock(), pending);
        self.flags.fetch_and(!mask_current, Ordering::Release);
        v.into_iter().for_each(|rd| unsafe { rd.release() });
    }
    pub fn rcu_assert(&self, id: usize) {
        debug_assert!(self.flags.load(Ordering::Relaxed) & (1 << id) != 0)
    }
    pub fn rcu_drop<T: RcuCollect>(&self, x: T) {
        self.rcu_drop_usize(unsafe { x.rcu_transmute() })
    }
    pub fn rcu_drop_usize(&self, v: RcuDrop) {
        self.rcu_pending.lock().push(v)
    }
    pub fn rcu_drop_group(&self, v: &mut Vec<RcuDrop>) {
        self.rcu_pending.lock().append(v);
    }
}
