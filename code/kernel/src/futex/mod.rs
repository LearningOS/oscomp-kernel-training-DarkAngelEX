use core::{
    ptr::NonNull,
    sync::atomic::{AtomicPtr, Ordering},
};

use alloc::{
    collections::BTreeMap,
    sync::{Arc, Weak},
};
use ftl_util::rcu::RcuCollect;

use crate::{
    memory::{address::UserAddr4K, user_ptr::UserInOutPtr},
    process::thread::Thread,
    sync::mutex::SpinNoIrqLock,
    timer::TimeTicks,
};

use self::queue::FutexQueue;

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
    #[inline]
    pub fn wake(&self, mask: u32, n: usize) -> usize {
        self.queue.lock().wake(mask, n)
    }
    #[inline]
    pub fn wake_all_close(&self) {
        self.queue.lock().wake_all_close();
    }
    #[inline]
    pub async fn wait(
        &self,
        mask: u32,
        timeout: TimeTicks,
        mut fail: impl FnMut() -> bool,
    ) -> WaitStatus {
        if self.closed() {
            return WaitStatus::Closed;
        }
        // 无锁数据预检测
        if fail() {
            return WaitStatus::Fail;
        }
        FutexQueue::wait(&self.queue, mask, timeout, fail).await
    }
}

/// 用于页面管理器的Futex对象
///
/// 使用RCU释放futex, 保证 ViewFutex 指针指向的内存必然有效
pub struct OwnFutex(Option<Arc<Futex>>);

impl Drop for OwnFutex {
    fn drop(&mut self) {
        let p = self.0.take().unwrap();
        p.wake_all_close();
        p.rcu_drop();
    }
}

impl OwnFutex {
    pub fn new() -> Self {
        Self(Some(Arc::new(Futex::new())))
    }
    fn assume_some(&self) {
        debug_assert!(self.0.is_some());
        unsafe { core::intrinsics::assume(self.0.is_some()) }
    }
    pub fn take_weak(&self) -> Weak<Futex> {
        self.assume_some();
        Arc::downgrade(&self.0.as_ref().unwrap())
    }
    fn take_view(&self) -> ViewFutex {
        self.assume_some();
        ViewFutex(AtomicPtr::new(
            self.0.as_ref().unwrap() as *const _ as *mut _
        ))
    }
}

/// 用于页表管理器的futex集合
pub struct FutexSet(BTreeMap<UserAddr4K, OwnFutex>);

/// only run when fork
impl Clone for FutexSet {
    fn clone(&self) -> Self {
        todo!()
    }
}

impl FutexSet {
    pub const fn new() -> Self {
        Self(BTreeMap::new())
    }
}

/// 被等待器持有的futex指针
///
///     0 -> 已发射
///     dangling -> 等待
///     Other -> 存在队列
///
/// 转移的过程:
///    ---A---    >>>>    ---B---
///    | lock |          | lock |
/// ptr:  A -->-- wait -->-- B
///
struct ViewFutex(AtomicPtr<Futex>);

enum ViewOp {
    Issued,
    Queued(*mut Futex),
    Waited,
}

impl ViewFutex {
    const ISSUED_V: *mut Futex = core::ptr::null_mut();
    const WAITED_V: *mut Futex = NonNull::<Futex>::dangling().as_ptr();
    #[inline]
    fn fetch(&self) -> ViewOp {
        let p = self.0.load(Ordering::Relaxed);
        if p == Self::ISSUED_V {
            ViewOp::Issued
        } else if p != Self::WAITED_V {
            ViewOp::Queued(p)
        } else {
            ViewOp::Waited // unlikely
        }
    }
    pub fn set_issued(&self) {
        debug_assert!(matches!(self.fetch(), ViewOp::Issued));
        self.0.store(Self::ISSUED_V, Ordering::Relaxed);
    }
    pub fn set_waited(&self) {
        debug_assert!(matches!(self.fetch(), ViewOp::Queued(_)));
        self.0.store(Self::WAITED_V, Ordering::Relaxed);
    }
    pub fn set_queued(&self, new: &Futex) {
        debug_assert!(matches!(self.fetch(), ViewOp::Waited));
        self.0.store(new as *const _ as *mut _, Ordering::Relaxed);
    }
    /// None: issued
    ///
    /// Some(p): queued
    #[inline]
    fn load_queue(&self) -> Option<*mut Futex> {
        #[cfg(debug_assertions)]
        let mut cnt = 0;
        loop {
            match self.fetch() {
                ViewOp::Issued => return None,
                ViewOp::Queued(p) => return Some(p),
                ViewOp::Waited => (),
            }
            #[cfg(debug_assertions)]
            {
                cnt += 1;
                assert!(cnt < 1000000, "ViewFutex deadlock");
            }
        }
    }
    /// 使用双重检查法获取锁并运行函数
    ///
    /// Ok(()): success lock
    ///
    /// Err(()): issued
    #[inline]
    pub fn lock_queue_run<T>(&self, f: impl FnOnce(&mut FutexQueue) -> T) -> Result<T, ()> {
        stack_trace!();
        // 猜测自己的队列指针是被使用的, 所有修改都要在获取锁的情况下进行!
        let mut p = self.load_queue().ok_or(())?;
        loop {
            let queue = &mut *unsafe { &*p }.queue.lock();
            let q = self.load_queue().ok_or(())?;
            if p != q {
                p = q;
                continue;
            }
            // 现在获取的锁和自身的队列是同一个
            debug_assert!(!queue.closed());
            return Ok(f(queue));
        }
    }
}

pub async fn wait(_thread: &Thread) {
    todo!()
}
