use core::{
    future::Future,
    ops::{Deref, DerefMut},
    pin::Pin,
    ptr::NonNull,
    task::{Context, Poll, Waker},
};

#[inline]
pub async fn take_waker() -> Waker {
    TakeWakerFuture.await
}

/// 避免一次Waker原子递增
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
