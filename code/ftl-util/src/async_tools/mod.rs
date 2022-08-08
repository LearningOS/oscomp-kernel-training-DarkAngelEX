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

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct WakerPtr(NonNull<Waker>);
impl WakerPtr {
    pub const fn dangling() -> Self {
        Self(NonNull::dangling())
    }
    pub fn new(waker: &Waker) -> Self {
        Self(NonNull::new(waker as *const _ as *mut _).unwrap())
    }
    pub fn wake(self) {
        debug_assert!(self.0 != NonNull::dangling());
        unsafe { self.0.as_ref().wake_by_ref() }
    }
}
unsafe impl Send for WakerPtr {}
unsafe impl Sync for WakerPtr {}

/// 此函数保证不会阻塞, 自旋锁可以安全跨越
#[inline(always)]
pub async fn take_waker() -> Waker {
    TakeWakerFuture.await
}

struct TakeWakerFuture;

impl Future for TakeWakerFuture {
    type Output = Waker;
    #[inline(always)]
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Ready(cx.waker().clone())
    }
}

pub struct SendWraper<T>(pub T);

impl<T> SendWraper<T> {
    /// # Safety
    ///
    /// 用户自行保证它不会跨越await
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
