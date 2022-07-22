pub mod arena;
pub mod tiny_env;

use core::{
    future::Future,
    ops::{Deref, DerefMut},
    pin::Pin,
    ptr::NonNull,
    task::{Context, Poll, Waker},
};

use alloc::boxed::Box;

use crate::error::{SysR, SysRet};

pub type Async<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub type ASysR<'a, T> = Async<'a, SysR<T>>;
pub type ASysRet<'a> = Async<'a, SysRet>;

/// 此函数保证不会阻塞, 自旋锁可以安全跨越
#[inline(always)]
pub async fn take_waker() -> Waker {
    TakeWakerFuture.await
}

/// 此函数保证不会阻塞, 自旋锁可以安全跨越
///
/// 相对take_waker可以避免一次Waker引用计数原子递增, 但需要注意生命周期
#[inline(always)]
pub async fn take_waker_ptr() -> NonNull<Waker> {
    TakeWakerPtrFuture.await
}

struct TakeWakerFuture;

impl Future for TakeWakerFuture {
    type Output = Waker;
    #[inline(always)]
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
    #[inline(always)]
    pub unsafe fn new(v: T) -> Self {
        SendWraper(v)
    }
}

unsafe impl<T> Send for SendWraper<T> {}

impl<T: Deref> Deref for SendWraper<T> {
    type Target = T::Target;
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}
impl<T: DerefMut> DerefMut for SendWraper<T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}
