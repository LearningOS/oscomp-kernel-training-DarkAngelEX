use core::ops::Range;

use alloc::{
    collections::BTreeMap,
    sync::{Arc, Weak},
};
use ftl_util::{rcu::RcuCollect, time::Instant};

use crate::{
    memory::{address::UserAddr, user_ptr::UserInOutPtr},
    process::Pid,
    sync::mutex::SpinNoIrqLock,
    tools::range::URange,
};

use self::queue::{FutexQueue, TempQueue};

mod queue;

pub const FUTEX_BITSET_MATCH_ANY: u32 = u32::MAX;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RobustList {
    pub next: UserInOutPtr<RobustList>,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RobustListHead {
    pub list: RobustList,
    pub futex_offset: usize,
    pub list_op_pending: UserInOutPtr<RobustList>,
}

#[must_use]
pub enum WaitStatus {
    /// 正常结束wait
    Ok,
    /// 内存检查失败
    Fail,
    /// 此futex已经被关闭, 需要重新获取
    Closed,
}

#[must_use]
pub enum WakeStatus {
    /// 正常结束wait
    Ok(usize),
    /// 内存检查失败
    Fail,
    /// 此futex已经被关闭, 需要重新获取
    Closed,
}

pub struct Futex {
    queue: SpinNoIrqLock<FutexQueue>,
}

impl Futex {
    #[inline]
    pub fn new() -> Self {
        Self {
            queue: SpinNoIrqLock::new(FutexQueue::new()),
        }
    }
    #[inline]
    pub fn init(&mut self) {
        self.queue.get_mut().init();
    }
    #[inline]
    pub fn closed(&self) -> bool {
        unsafe { self.queue.unsafe_get().closed() }
    }
    pub fn wake(
        &self,
        mask: u32,
        max: usize,
        pid: Option<Pid>,
        mut fail: impl FnMut() -> bool,
    ) -> WakeStatus {
        if self.closed() {
            return WakeStatus::Closed;
        }
        if fail() {
            return WakeStatus::Fail;
        }
        self.queue.lock().wake(mask, max, pid, fail)
    }
    pub fn wake_requeue(
        &self,
        max_wake: usize,
        max_requeue: usize,
        pid: Option<Pid>,
        mut fail: impl FnMut() -> bool,
    ) -> (WakeStatus, Option<TempQueue>) {
        if self.closed() {
            return (WakeStatus::Closed, None);
        }
        if fail() {
            return (WakeStatus::Fail, None);
        }
        self.queue
            .lock()
            .wake_requeue(max_wake, max_requeue, pid, fail)
    }
    // 返回Err表明futex已经关闭, 不会对q产生任何修改
    pub fn append(&self, q: &mut TempQueue) -> Result<(), ()> {
        if self.closed() {
            return Err(());
        }
        self.queue.lock().append(q, self)
    }
    #[inline]
    pub fn wake_all_close(&self) {
        self.queue.lock().wake_all_close();
    }
    pub async fn wait(
        &self,
        mask: u32,
        timeout: Instant,
        pid: Option<Pid>,
        mut fail: impl FnMut() -> bool,
    ) -> WaitStatus {
        if self.closed() {
            return WaitStatus::Closed;
        }
        // 无锁数据预检测
        if fail() {
            return WaitStatus::Fail;
        }
        FutexQueue::wait(self, mask, timeout, pid, fail).await
    }
}

/// 用于页面管理器的Futex对象
///
/// 使用RCU释放futex, 保证 ViewFutex 指针指向的内存必然有效
///
/// (futex, process_private)
#[derive(Clone)]
pub struct OwnFutex(Option<Arc<Futex>>, bool);

impl Drop for OwnFutex {
    fn drop(&mut self) {
        let p = self.0.take().unwrap();
        p.wake_all_close();
        p.rcu_drop();
    }
}

impl OwnFutex {
    pub fn new(private: bool) -> Self {
        let mut fx = Arc::new(Futex::new());
        unsafe { Arc::get_mut_unchecked(&mut fx).init() }
        Self(Some(fx), private)
    }
    pub fn private(&self) -> bool {
        self.1
    }
    fn assume_some(&self) {
        debug_assert!(self.0.is_some());
        unsafe { core::intrinsics::assume(self.0.is_some()) }
    }
    pub fn take_arc(&self) -> Arc<Futex> {
        self.assume_some();
        debug_assert!(!self.0.as_ref().unwrap().closed());
        self.0.as_ref().unwrap().clone()
    }
}

/// 用于页表管理器的futex集合
pub struct FutexSet(BTreeMap<UserAddr<u32>, OwnFutex>);

impl FutexSet {
    pub const fn new() -> Self {
        Self(BTreeMap::new())
    }
    /// if futex does not exist in ua, create a new one by private flag
    ///
    /// this is the only way to create futex
    pub fn fetch_create(
        &mut self,
        ua: UserAddr<u32>,
        private: impl FnOnce() -> bool,
    ) -> &mut OwnFutex {
        debug_assert!(ua.is_align());
        self.0
            .entry(ua)
            .or_insert_with(move || OwnFutex::new(private()))
    }
    pub fn try_fetch(&mut self, ua: UserAddr<u32>) -> Option<&mut OwnFutex> {
        debug_assert!(ua.is_align());
        self.0.get_mut(&ua)
    }
    pub fn remove(&mut self, URange { start, end }: URange) {
        if self.0.is_empty() {
            return;
        }
        let r: Range<UserAddr<u32>> = start.into()..end.into();
        let cnt = self.0.range(r.clone()).map(|(&k, _)| k).count();
        if cnt == 0 {
            return;
        } else if cnt == self.0.len() {
            self.0.clear();
        } else if cnt * 4 > self.0.len() {
            self.0.retain(move |&k, _| k < r.start || k >= r.end);
        } else {
            while let Some((&k, _)) = self.0.range(r.clone()).next() {
                drop(self.0.remove(&k).unwrap());
            }
        }
    }
    pub fn clear(&mut self) {
        self.0.clear();
    }
    pub fn fork(&self) -> Self {
        let new: BTreeMap<UserAddr<u32>, OwnFutex> = self
            .0
            .iter()
            .filter(|(_, v)| !v.private())
            .map(|(&k, v)| (k, v.clone()))
            .collect();
        Self(new)
    }
}

pub struct FutexIndex(BTreeMap<UserAddr<u32>, Weak<Futex>>);

impl FutexIndex {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }
    /// 此函数会无锁确认closed标志位
    pub fn try_fetch(&self, ua: UserAddr<u32>) -> Option<Arc<Futex>> {
        debug_assert!(ua.is_align());
        self.0
            .get(&ua)
            .and_then(|p| p.upgrade())
            .filter(|a| !a.closed())
    }
    pub fn insert(&mut self, ua: UserAddr<u32>, v: Weak<Futex>) {
        debug_assert!(ua.is_align());
        drop(self.0.insert(ua, v));
    }
    pub fn garbage_collection(&mut self) {
        self.0.retain(|_, v| v.strong_count() != 0);
    }
    pub fn fork(&mut self) -> Self {
        self.garbage_collection();
        Self(self.0.clone())
    }
}
