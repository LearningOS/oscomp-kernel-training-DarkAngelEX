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

#[repr(transparent)]
pub struct SendWraper<T>(pub T);

impl<T> SendWraper<T> {
    /// # Safety
    ///
    /// 用户自行保证它不会跨越await
    #[inline(always)]
    pub unsafe fn new(v: T) -> Self {
        SendWraper(v)
    }
    pub fn map<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        f(&self.0)
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

pub enum Join2R<T1, T2> {
    First(T1),
    Second(T2),
}
pub struct Join2Future<T1, T2, F1: Future<Output = T1>, F2: Future<Output = T2>>(pub F1, pub F2);

impl<T1, T2, F1: Future<Output = T1>, F2: Future<Output = T2>> Future
    for Join2Future<T1, T2, F1, F2>
{
    type Output = Join2R<T1, T2>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        if let Poll::Ready(r) = unsafe { Pin::new_unchecked(&mut this.0).poll(cx) } {
            Poll::Ready(Join2R::First(r))
        } else if let Poll::Ready(r) = unsafe { Pin::new_unchecked(&mut this.1).poll(cx) } {
            Poll::Ready(Join2R::Second(r))
        } else {
            Poll::Pending
        }
    }
}
