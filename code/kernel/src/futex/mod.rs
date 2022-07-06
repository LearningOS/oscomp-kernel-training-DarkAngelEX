use core::ops::DerefMut;

use alloc::sync::{Arc, Weak};
use ftl_util::rcu::RcuCollect;

use crate::{
    memory::user_ptr::UserInOutPtr, process::thread::Thread, sync::mutex::SpinNoIrqLock,
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
pub struct OwnFutex(Arc<Futex>);

impl Drop for OwnFutex {
    fn drop(&mut self) {
        self.0.wake_all_close();
    }
}

impl OwnFutex {
    pub fn new() -> Self {
        Self(Arc::new(Futex::new()))
    }
    pub fn take_weak(&self) -> Weak<Futex> {
        Arc::downgrade(&self.0)
    }
    fn take_view(&self) -> ViewFutex {
        ViewFutex(Some(self.0.clone()))
    }
}

struct ViewFutex(Option<Arc<Futex>>);

impl ViewFutex {
    /// 如果为无效值则返回None, 需要等待其他线程操作完成
    pub fn lock(&self) -> Option<impl DerefMut<Target = FutexQueue> + '_> {
        self.0.as_ref().map(|a| a.queue.lock())
    }
    pub fn replace(&mut self, new: Arc<Futex>) {
        core::mem::replace(&mut self.0, Some(new)).map(|a| a.rcu_drop());
    }
    pub fn set_invalid(&mut self) {
        self.0.take().map(|a| a.rcu_drop());
    }
    pub fn set_queue(&mut self, new: Arc<Futex>) {
        debug_assert!(self.0.is_none());
        self.0 = Some(new);
    }
}

pub async fn wait(_thread: &Thread) {
    todo!()
}
