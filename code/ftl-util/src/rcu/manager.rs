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
/// flags划分为2部分, [63:32|31:0] = [current|pending]
///     current: rcu_current 临界区运行的hart
///     pending: 正在临界区运行的hart
///
/// 发起临界区:
/// flags_pending 设置CPU对应位 表示 rcu_pending 包含本CPU的临界区
///
/// 离开临界区:
/// flags_current 删除对应CPU位并判断, 如果为0则:
///     转移 flags_pending 至 flags_current, 增加当前CPU位防止顺序错误
///     释放 rcu_current, 转移 rcu_pending 至 rcu_current
///     删除当前CPU位, 这期间收集到的内存留给下个释放周期释放
///
pub struct RcuManager<S: MutexSupport> {
    flags: AtomicU64,
    cp: SpinMutex<CP, S>,
}

pub struct CP {
    rcu_current: Vec<RcuDrop>,
    rcu_pending: Vec<RcuDrop>,
}

impl CP {
    fn cp_mut(&mut self) -> (&mut Vec<RcuDrop>, &mut Vec<RcuDrop>) {
        (&mut self.rcu_current, &mut self.rcu_pending)
    }
}

impl<S: MutexSupport> RcuManager<S> {
    pub const fn new() -> Self {
        Self {
            flags: AtomicU64::new(0),
            cp: SpinMutex::new(CP {
                rcu_current: Vec::new(),
                rcu_pending: Vec::new(),
            }),
        }
    }
    pub fn critical_start(&self, id: usize) {
        debug_assert!(id < 32);
        let mask_pending = 1 << id;
        if self.flags.load(Ordering::Relaxed) & mask_pending == 0 {
            self.flags.fetch_or(mask_pending, Ordering::Relaxed);
        }
    }
    /// 一个核结束了临界区, 并将这期间的释放队列提交到这里
    ///
    /// add: 当前核心的释放队列, 按情况提交到 pending 或 current
    pub fn critical_end(&self, id: usize, add: &mut Vec<RcuDrop>) {
        debug_assert!(id < 32);
        let mask_pending = 1 << id;
        let mask_current = mask_pending << 32;
        let mask_all = mask_pending | mask_current;
        let mut release; // rcu逻辑上释放
        let mut need_release; // 真的需要释放东西(队列不为空)
        let mut prev = self.flags.load(Ordering::Relaxed);
        if prev & mask_all == 0 {
            if !add.is_empty() {
                fast_append(&mut self.cp.lock().rcu_pending, add);
            }
            return;
        }
        loop {
            let mut next = prev & !mask_all;
            release = (next >> 32) == 0; // current 为 0 说明要释放了
            let cp = unsafe { self.cp.unsafe_get() };
            need_release = !cp.rcu_pending.is_empty() || !cp.rcu_current.is_empty();
            if release {
                next <<= 32;
                if need_release {
                    next |= mask_current; // 锁定RCU管理器
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
        if !release {
            if !add.is_empty() {
                fast_append(&mut self.cp.lock().rcu_pending, add);
            }
            return;
        }
        if !need_release {
            // 绕过锁
            if !add.is_empty() {
                fast_append(&mut self.cp.lock().rcu_current, add);
            }
            return;
        }
        // 现在 RCU 释放队列已经被锁定, 保证其他核不会介入释放过程

        // add 和 pending 转移到 current, current 转移到 add
        let mut cp = self.cp.lock();
        let (c, p) = cp.cp_mut();
        vec_swap(add, c);
        fast_append(c, p);
        drop(cp);
        self.flags.fetch_and(!mask_current, Ordering::Relaxed);
        for rd in add.drain(..) {
            unsafe { rd.release() }
        }
        debug_assert!(add.is_empty());
    }
    pub fn rcu_assert(&self, id: usize) {
        debug_assert!(self.flags.load(Ordering::Relaxed) & (1 << id) != 0)
    }
    pub fn rcu_drop<T: RcuCollect>(&self, x: T) {
        self.rcu_drop_usize(unsafe { x.rcu_transmute() })
    }
    pub fn rcu_drop_usize(&self, v: RcuDrop) {
        self.cp.lock().rcu_pending.push(v)
    }
    pub fn rcu_drop_group(&self, v: &mut Vec<RcuDrop>) {
        self.cp.lock().rcu_pending.append(v);
    }
}

/// vec为3个usize, swap_nonoverlapping 比 swap 更快
#[allow(clippy::ptr_arg)]
fn vec_swap<T>(a: &mut Vec<T>, b: &mut Vec<T>) {
    unsafe {
        core::ptr::swap_nonoverlapping(a, b, 1);
    }
}

#[allow(clippy::ptr_arg)]
fn fast_append<T>(dst: &mut Vec<T>, src: &mut Vec<T>) {
    if dst.len() + 20 < src.len() {
        vec_swap(dst, src);
    }
    dst.append(src);
}
