use core::{
    cell::UnsafeCell,
    future::Future,
    ops::{Deref, DerefMut},
    pin::Pin,
    task::{Context, Poll, Waker},
};

use crate::{async_tools, list::ListNode};

use super::{spin_mutex::SpinMutex, MutexSupport};

pub struct SleepMutex<T: ?Sized, S: MutexSupport> {
    lock: SpinMutex<ListNode<(bool, Option<Waker>)>, S>, // push at prev, release at next
    data: UnsafeCell<T>,                                 // actual data
}

unsafe impl<T: ?Sized + Send, S: MutexSupport> Send for SleepMutex<T, S> {}
unsafe impl<T: ?Sized + Send, S: MutexSupport> Sync for SleepMutex<T, S> {}

impl<T, S: MutexSupport> SleepMutex<T, S> {
    pub const fn new(user_data: T) -> Self {
        Self {
            lock: SpinMutex::new(ListNode::new((false, None))),
            data: UnsafeCell::new(user_data),
        }
    }
    pub fn into_inner(self) -> T {
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
    pub async fn lock(&self) -> impl DerefMut<Target = T> + Send + Sync + '_ {
        let mut node = ListNode::new((false, None));
        let mut future = SleepLockFuture::new(self, &mut node);
        future.init().await;
        future.await
    }
}

struct SleepLockFuture<'a, 'b, T: ?Sized, S: MutexSupport> {
    mutex: &'a SleepMutex<T, S>,
    node: &'b mut ListNode<(bool, Option<Waker>)>,
}

impl<'a, 'b, T: ?Sized, S: MutexSupport> SleepLockFuture<'a, 'b, T, S> {
    fn new(mutex: &'a SleepMutex<T, S>, node: &'b mut ListNode<(bool, Option<Waker>)>) -> Self {
        SleepLockFuture { mutex, node }
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
        mx_list.push_prev(self.node);
    }
}

impl<'a, 'b, T: ?Sized, S: MutexSupport> Future for SleepLockFuture<'a, 'b, T, S> {
    type Output = SleepMutexGuard<'a, T, S>;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        let ptr = &self.node.data().0;
        match *ptr {
            false => Poll::Pending,
            true => Poll::Ready(SleepMutexGuard { mutex: self.mutex }),
        }
    }
}

struct SleepMutexGuard<'a, T: ?Sized, S: MutexSupport> {
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
        let next = match mx_list.pop_next() {
            None => {
                mx_list.data_mut().0 = false; // unlock
                return;
            }
            Some(mut next) => unsafe { next.as_mut().data_mut() },
        };
        drop(mx_list);
        // waker必须在 next.0 = true 之前获取, 在这之后next将失效
        let waker = next.1.take().unwrap();
        next.0 = true;
        waker.wake();
    }
}
