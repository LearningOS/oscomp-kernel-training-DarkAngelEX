use core::{
    cell::UnsafeCell,
    future::Future,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    pin::Pin,
    task::{Context, Poll, Waker},
};

use crate::{async_tools, list::SyncListNode};

use super::{spin_mutex::SpinMutex, MutexSupport};

pub struct SleepMutex<T: ?Sized, S: MutexSupport> {
    lock: SpinMutex<SyncListNode<(bool, Option<Waker>)>, S>, // push at prev, release at next
    _marker: PhantomData<S>,
    data: UnsafeCell<T>, // actual data
}

unsafe impl<T: ?Sized + Send, S: MutexSupport> Send for SleepMutex<T, S> {}
unsafe impl<T: ?Sized + Send, S: MutexSupport> Sync for SleepMutex<T, S> {}

impl<T, S: MutexSupport> SleepMutex<T, S> {
    pub const fn new(user_data: T) -> Self {
        Self {
            lock: SpinMutex::new(SyncListNode::new((false, None))),
            data: UnsafeCell::new(user_data),
            _marker: PhantomData,
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
impl<T: ?Sized + Send, S: MutexSupport> SleepMutex<T, S> {
    pub fn get_mut(&mut self) -> &mut T {
        self.data.get_mut()
    }
    pub unsafe fn unsafe_get(&self) -> &T {
        &*self.data.get()
    }
    pub unsafe fn unsafe_get_mut(&self) -> &mut T {
        &mut *self.data.get()
    }
    #[inline(always)]
    pub async fn lock(&self) -> impl DerefMut<Target = T> + Send + Sync + '_ {
        let mut future = SleepLockFuture::new(self);
        future.init().await;
        future.await
    }
}

struct SleepLockFuture<'a, T: ?Sized, S: MutexSupport> {
    mutex: &'a SleepMutex<T, S>,
    node: SyncListNode<(bool, Option<Waker>)>,
}

impl<'a, T: ?Sized, S: MutexSupport> SleepLockFuture<'a, T, S> {
    fn new(mutex: &'a SleepMutex<T, S>) -> Self {
        SleepLockFuture {
            mutex,
            node: SyncListNode::new((false, None)),
        }
    }
    async fn init(&mut self) {
        self.node.init();
        let mx_list = unsafe { &mut *self.mutex.lock.send_lock() };
        mx_list.lazy_init();
        let this = self.node.data_mut();
        let head = mx_list.data_mut();
        // empty
        if !head.0 {
            head.0 = true; // lock
            this.0 = true;
            return;
        }
        this.1 = Some(async_tools::take_waker().await);
        mx_list.insert_prev(&mut self.node);
    }
}

impl<'a, T: ?Sized, S: MutexSupport> Future for SleepLockFuture<'a, T, S> {
    type Output = SleepMutexGuard<'a, T, S>;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.node.data().0 {
            false => Poll::Pending,
            true => Poll::Ready(SleepMutexGuard { mutex: self.mutex }),
        }
    }
}

struct SleepMutexGuard<'a, T: ?Sized + 'a, S: MutexSupport> {
    mutex: &'a SleepMutex<T, S>,
}

unsafe impl<'a, T: ?Sized + Send, S: MutexSupport> Send for SleepMutexGuard<'a, T, S> {}
unsafe impl<'a, T: ?Sized + Send, S: MutexSupport> Sync for SleepMutexGuard<'a, T, S> {}

impl<'a, T: ?Sized, S: MutexSupport> Deref for SleepMutexGuard<'a, T, S> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.mutex.data.get() }
    }
}

impl<'a, T: ?Sized, S: MutexSupport> DerefMut for SleepMutexGuard<'a, T, S> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<'a, T: ?Sized, S: MutexSupport> Drop for SleepMutexGuard<'a, T, S> {
    fn drop(&mut self) {
        let mut mx_list = self.mutex.lock.lock();
        debug_assert!(mx_list.data_mut().0);
        let next = match mx_list.try_remove_next() {
            None => {
                mx_list.data_mut().0 = false;
                return;
            }
            Some(mut next) => unsafe { next.as_mut().data_mut() },
        };
        drop(mx_list);
        // waker必须在 next.0 = true 之前获取, 在这之后next将失效
        let waker = next.1.take().unwrap();
        super::seq_fence();
        next.0 = true;
        waker.wake();
    }
}
