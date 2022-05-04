use core::{
    future::Future,
    ops::{Deref, DerefMut},
    pin::Pin,
    ptr::NonNull,
    task::{Context, Poll, Waker},
};

/// 此函数保证不会阻塞, 自旋锁可以安全跨越
#[inline]
pub async fn take_waker() -> Waker {
    TakeWakerFuture.await
}

/// 此函数保证不会阻塞, 自旋锁可以安全跨越
///
/// 相对take_waker可以避免一次Waker引用计数原子递增, 但需要注意生命周期
#[inline]
pub async fn take_waker_ptr() -> NonNull<Waker> {
    TakeWakerPtrFuture.await
}

struct TakeWakerFuture;

impl Future for TakeWakerFuture {
    type Output = Waker;
    #[inline]
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Ready(cx.waker().clone())
    }
}

struct TakeWakerPtrFuture;

impl Future for TakeWakerPtrFuture {
    type Output = NonNull<Waker>;
    #[inline]
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Ready(cx.waker().into())
    }
}

pub struct SendWraper<T>(T);

impl<T> SendWraper<T> {
    pub unsafe fn new(v: T) -> Self {
        SendWraper(v)
    }
}

unsafe impl<T> Send for SendWraper<T> {}

impl<T: Deref> Deref for SendWraper<T> {
    type Target = T::Target;
    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}
impl<T: DerefMut> DerefMut for SendWraper<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}
