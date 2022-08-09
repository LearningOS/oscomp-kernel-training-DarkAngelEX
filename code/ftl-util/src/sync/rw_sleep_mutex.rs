use core::{
    cell::UnsafeCell,
    future::Future,
    ops::{Deref, DerefMut},
    pin::Pin,
    task::{Context, Poll, Waker},
};

use crate::{async_tools, list::ListNode};

use super::{spin_mutex::SpinMutex, MutexSupport};

type ListNodeImpl = ListNode<(bool, Option<Waker>)>;

/// 按共享 - 排他 - 共享 - 排他 顺序释放任务
///
pub struct RwSleepMutex<T: ?Sized, S: MutexSupport> {
    /// head.usize: 0 any
    ///
    /// wait.usize: 0 pending 1 ready
    ///
    lock: SpinMutex<MutexInner, S>, // push at prev, release at next
    data: UnsafeCell<T>, // actual data
}

enum Status {
    Unlock,
    Unique,
    Shared(usize),
}

impl Status {
    pub fn shared_count(&self) -> Option<usize> {
        match *self {
            Self::Shared(n) => Some(n),
            _ => None,
        }
    }
}

struct MutexInner {
    this_ptr: usize,
    status: Status,
    shared: ListNodeImpl,
    unique: ListNodeImpl,
}
impl MutexInner {
    const fn new() -> Self {
        Self {
            this_ptr: 0,
            status: Status::Unlock,
            shared: ListNodeImpl::new((false, None)),
            unique: ListNodeImpl::new((false, None)),
        }
    }
    fn lazy_init(&mut self) {
        if self.this_ptr != self as *mut _ as usize {
            self.shared.init();
            self.unique.init();
            self.this_ptr = self as *mut _ as usize;
        }
    }
}

unsafe impl<T: ?Sized + Send, S: MutexSupport> Send for RwSleepMutex<T, S> {}
unsafe impl<T: ?Sized + Send, S: MutexSupport> Sync for RwSleepMutex<T, S> {}

impl<T, S: MutexSupport> RwSleepMutex<T, S> {
    pub const fn new(user_data: T) -> Self {
        Self {
            lock: SpinMutex::new(MutexInner::new()),
            data: UnsafeCell::new(user_data),
        }
    }
    /// Consumes this mutex, returning the underlying data.
    pub fn into_inner(self) -> T {
        // We know statically that there are no outstanding references to
        // `self` so there's no need to lock.
        let Self { data, .. } = self;
        data.into_inner()
    }
}
impl<T: ?Sized, S: MutexSupport> RwSleepMutex<T, S> {
    pub fn get_mut(&mut self) -> &mut T {
        self.data.get_mut()
    }
    /// # Safety
    ///
    /// 自行保证数据访问的安全
    pub unsafe fn unsafe_get(&self) -> &T {
        &*self.data.get()
    }
    /// # Safety
    ///
    /// 自行保证数据访问的安全
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn unsafe_get_mut(&self) -> &mut T {
        &mut *self.data.get()
    }
}
impl<T: ?Sized + Send, S: MutexSupport> RwSleepMutex<T, S> {
    pub async fn shared_lock(&self) -> impl Deref<Target = T> + Send + Sync + '_ {
        let future = &mut SharedSleepLockFuture::new(self);
        unsafe { Pin::new_unchecked(future).init().await.await }
    }
    #[inline(always)]
    pub async fn unique_lock(&self) -> impl DerefMut<Target = T> + Send + Sync + '_ {
        let future = &mut UniqueSleepLockFuture::new(self);
        unsafe { Pin::new_unchecked(future).init().await.await }
    }
    pub fn try_shared_lock(&self) -> Option<impl Deref<Target = T> + Send + Sync + '_> {
        let mut lk = self.lock.lock();
        let n = match lk.status {
            Status::Unlock => 1,
            Status::Shared(n) if lk.unique.is_empty() => n + 1,
            _ => return None,
        };
        lk.status = Status::Shared(n);
        lk.lazy_init();
        Some(SharedSleepMutexGuard { mutex: self })
    }
    pub fn try_unique_lock(&self) -> Option<impl DerefMut<Target = T> + Send + Sync + '_> {
        let mut lk = self.lock.lock();
        match lk.status {
            Status::Unlock => (),
            _ => return None,
        }
        lk.status = Status::Unique;
        lk.lazy_init();
        Some(UnqiueSleepMutexGuard { mutex: self })
    }
}

struct SharedSleepLockFuture<'a, T: ?Sized, S: MutexSupport> {
    mutex: &'a RwSleepMutex<T, S>,
    node: ListNodeImpl,
}

impl<'a, T: ?Sized, S: MutexSupport> SharedSleepLockFuture<'a, T, S> {
    fn new(mutex: &'a RwSleepMutex<T, S>) -> Self {
        Self {
            mutex,
            node: ListNodeImpl::new((false, None)),
        }
    }
    async fn init(self: Pin<&mut Self>) -> Pin<&mut SharedSleepLockFuture<'a, T, S>> {
        stack_trace!();
        let this = unsafe { self.get_unchecked_mut() };
        this.node.init();
        let mx_list = unsafe { &mut *this.mutex.lock.send_lock() };
        mx_list.lazy_init();
        let data = this.node.data_mut();
        match mx_list.status {
            Status::Unlock => {
                mx_list.status = Status::Shared(1);
                data.0 = true;
            }
            Status::Shared(n) if mx_list.unique.is_empty() => {
                mx_list.status = Status::Shared(n + 1);
                data.0 = true;
            }
            _ => {
                data.1 = Some(async_tools::take_waker().await);
                mx_list.shared.push_prev(&mut this.node);
            }
        };
        unsafe { Pin::new_unchecked(this) }
    }
}

impl<'a, T: ?Sized, S: MutexSupport> Future for SharedSleepLockFuture<'a, T, S> {
    type Output = SharedSleepMutexGuard<'a, T, S>;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.node.data().0 {
            false => Poll::Pending,
            true => Poll::Ready(SharedSleepMutexGuard { mutex: self.mutex }),
        }
    }
}

struct UniqueSleepLockFuture<'a, T: ?Sized, S: MutexSupport> {
    mutex: &'a RwSleepMutex<T, S>,
    node: ListNodeImpl,
}

impl<'a, T: ?Sized, S: MutexSupport> UniqueSleepLockFuture<'a, T, S> {
    fn new(mutex: &'a RwSleepMutex<T, S>) -> Self {
        UniqueSleepLockFuture {
            mutex,
            node: ListNodeImpl::new((false, None)),
        }
    }
    async fn init(self: Pin<&mut Self>) -> Pin<&mut UniqueSleepLockFuture<'a, T, S>> {
        stack_trace!();
        let this = unsafe { self.get_unchecked_mut() };
        this.node.init();
        let mx_list = unsafe { &mut *this.mutex.lock.send_lock() };
        mx_list.lazy_init();
        let data = this.node.data_mut();
        match mx_list.status {
            Status::Unlock => {
                mx_list.status = Status::Unique;
                data.0 = true;
            }
            _ => {
                data.1 = Some(async_tools::take_waker().await);
                mx_list.unique.push_prev(&mut this.node);
            }
        }
        unsafe { Pin::new_unchecked(this) }
    }
}

impl<'a, T: ?Sized, S: MutexSupport> Future for UniqueSleepLockFuture<'a, T, S> {
    type Output = UnqiueSleepMutexGuard<'a, T, S>;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.node.data().0 {
            false => Poll::Pending,
            true => Poll::Ready(UnqiueSleepMutexGuard { mutex: self.mutex }),
        }
    }
}

struct SharedSleepMutexGuard<'a, T: ?Sized, S: MutexSupport> {
    mutex: &'a RwSleepMutex<T, S>,
}
// 禁止Mutex跨越await导致死锁或无意义阻塞
unsafe impl<'a, T: ?Sized + Send, S: MutexSupport> Send for SharedSleepMutexGuard<'a, T, S> {}
unsafe impl<'a, T: ?Sized + Send, S: MutexSupport> Sync for SharedSleepMutexGuard<'a, T, S> {}

impl<'a, T: ?Sized, S: MutexSupport> Deref for SharedSleepMutexGuard<'a, T, S> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.mutex.data.get() }
    }
}

impl<'a, T: ?Sized, S: MutexSupport> Drop for SharedSleepMutexGuard<'a, T, S> {
    fn drop(&mut self) {
        stack_trace!();
        let mut mx_list = self.mutex.lock.lock();
        match mx_list.status.shared_count().unwrap() {
            0 => panic!(),
            1 => (),
            n => {
                mx_list.status = Status::Shared(n - 1);
                return;
            }
        }
        if let Some(mut unique) = mx_list.unique.pop_next() {
            mx_list.status = Status::Unique;
            drop(mx_list);
            let data = unsafe { unique.as_mut().data_mut() };
            debug_assert!(!data.0);
            let waker = data.1.take().unwrap();
            super::seq_fence();
            data.0 = true;
            waker.wake();
            return;
        }
        // release all shared
        let mut cnt = 0;
        while let Some(mut shared) = mx_list.shared.pop_next() {
            let shared = unsafe { shared.as_mut().data_mut() };
            let waker = shared.1.take().unwrap();
            super::seq_fence();
            shared.0 = true;
            waker.wake();
            cnt += 1;
        }
        mx_list.status = match cnt {
            0 => Status::Unlock,
            _ => Status::Shared(cnt),
        };
    }
}

struct UnqiueSleepMutexGuard<'a, T: ?Sized, S: MutexSupport> {
    mutex: &'a RwSleepMutex<T, S>,
}
// 禁止Mutex跨越await导致死锁或无意义阻塞
unsafe impl<'a, T: ?Sized + Send, S: MutexSupport> Send for UnqiueSleepMutexGuard<'a, T, S> {}
unsafe impl<'a, T: ?Sized + Send, S: MutexSupport> Sync for UnqiueSleepMutexGuard<'a, T, S> {}

impl<'a, T: ?Sized, S: MutexSupport> Deref for UnqiueSleepMutexGuard<'a, T, S> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.mutex.data.get() }
    }
}

impl<'a, T: ?Sized, S: MutexSupport> DerefMut for UnqiueSleepMutexGuard<'a, T, S> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<'a, T: ?Sized, S: MutexSupport> Drop for UnqiueSleepMutexGuard<'a, T, S> {
    fn drop(&mut self) {
        stack_trace!();
        let mut mx_list = self.mutex.lock.lock();
        debug_assert!(matches!(mx_list.status, Status::Unique));
        // release all shared
        let mut cnt = 0;
        while let Some(mut shared) = mx_list.shared.pop_next() {
            let shared = unsafe { shared.as_mut().data_mut() };
            let waker = shared.1.take().unwrap();
            super::seq_fence();
            shared.0 = true;
            waker.wake();
            cnt += 1;
        }
        if cnt != 0 {
            mx_list.status = Status::Shared(cnt);
            return;
        }
        if let Some(mut unique) = mx_list.unique.pop_next() {
            mx_list.status = Status::Unique;
            drop(mx_list);
            let unique = unsafe { unique.as_mut().data_mut() };
            debug_assert!(!unique.0);
            let waker = unique.1.take().unwrap();
            super::seq_fence();
            unique.0 = true;
            waker.wake();
            return;
        }
        mx_list.status = Status::Unlock;
    }
}
