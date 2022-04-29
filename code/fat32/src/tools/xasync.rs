use core::{
    future::Future,
    pin::Pin,
    sync::atomic::{AtomicUsize, Ordering},
    task::{Context, Poll, Waker},
};

pub struct GetWakerFuture;
impl Future for GetWakerFuture {
    type Output = Waker;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Ready(cx.waker().clone())
    }
}

pub struct WaitSemFuture<'a>(pub &'a AtomicUsize);

impl Future for WaitSemFuture<'_> {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            let v = match self.0.load(Ordering::Relaxed) {
                0 => return Poll::Pending,
                v => v,
            };
            if self
                .0
                .compare_exchange(v, v - 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return Poll::Ready(());
            }
        }
    }
}

pub struct WaitingEventFuture<F: Fn() -> bool>(pub F);

impl<F: Fn() -> bool> Future for WaitingEventFuture<F> {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.0() {
            false => Poll::Pending,
            true => Poll::Ready(()),
        }
    }
}
