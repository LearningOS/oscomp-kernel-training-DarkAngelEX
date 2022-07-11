use core::{
    cmp::{Ordering, Reverse},
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
    time::Duration,
};

use alloc::collections::BinaryHeap;
use ftl_util::{async_tools, error::SysError};

use crate::{
    process::thread,
    sync::{
        even_bus::{Event, EventBus},
        mutex::SpinNoIrqLock,
    },
    syscall::SysResult,
};

struct TimerCondVar {
    timeout: Duration,
    waker: Waker,
}
impl TimerCondVar {
    pub fn new(timeout: Duration, waker: Waker) -> Self {
        Self { timeout, waker }
    }
}

impl PartialEq for TimerCondVar {
    fn eq(&self, other: &Self) -> bool {
        self.timeout == other.timeout
    }
}

impl Eq for TimerCondVar {}
impl PartialOrd for TimerCondVar {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.timeout.cmp(&other.timeout))
    }
}
impl Ord for TimerCondVar {
    fn cmp(&self, other: &Self) -> Ordering {
        self.timeout.cmp(&other.timeout)
    }
}

struct SleepQueue {
    queue: BinaryHeap<Reverse<TimerCondVar>>,
}
impl SleepQueue {
    pub fn new() -> Self {
        Self {
            queue: BinaryHeap::new(),
        }
    }
    pub fn ignore(timeout: Duration) -> bool {
        timeout.as_secs() >= i64::MAX as u64
    }
    pub fn push(&mut self, ticks: Duration, waker: Waker) {
        if Self::ignore(ticks) {
            return;
        }
        self.queue.push(Reverse(TimerCondVar::new(ticks, waker)))
    }
    pub fn check_timer(&mut self, current: Duration) {
        stack_trace!();
        while let Some(Reverse(v)) = self.queue.peek() {
            if v.timeout <= current {
                self.queue.pop().unwrap().0.waker.wake();
            } else {
                break;
            }
        }
    }
}

static SLEEP_QUEUE: SpinNoIrqLock<Option<SleepQueue>> = SpinNoIrqLock::new(None);

pub fn sleep_queue_init() {
    *SLEEP_QUEUE.lock() = Some(SleepQueue::new());
}

pub fn timer_push_task(ticks: Duration, waker: Waker) {
    if SleepQueue::ignore(ticks) {
        return;
    }
    SLEEP_QUEUE.lock().as_mut().unwrap().push(ticks, waker);
}

pub fn check_timer() {
    stack_trace!();
    let current = super::get_time();
    SLEEP_QUEUE.lock().as_mut().unwrap().check_timer(current);
}

pub async fn just_wait(dur: Duration) {
    let mut future = JustWaitFuture::new(dur);
    let mut ptr = Pin::new(&mut future);
    ptr.as_mut().init().await;
    ptr.await
}

struct JustWaitFuture(Duration);
impl JustWaitFuture {
    pub fn new(dur: Duration) -> Self {
        Self(super::get_time() + dur)
    }
    pub async fn init(self: Pin<&mut Self>) {
        thread::yield_now().await;
        if self.0 <= super::get_time() {
            return;
        }
        let waker = async_tools::take_waker().await;
        self::timer_push_task(self.0, waker);
    }
}

impl Future for JustWaitFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.0 <= super::get_time() {
            return Poll::Ready(());
        }
        Poll::Pending
    }
}

pub async fn sleep(dur: Duration, event_bus: &EventBus) -> SysResult {
    let deadline = super::get_time() + dur;
    thread::yield_now().await;
    if super::get_time() >= deadline {
        return Ok(0);
    }
    if event_bus.event() != Event::empty() {
        return Err(SysError::EINTR);
    }
    let mut future = SleepFuture::new(deadline, event_bus);
    let mut ptr = Pin::new(&mut future);
    ptr.as_mut().init().await;
    ptr.await
}

struct SleepFuture<'a> {
    deadline: Duration,
    event_bus: &'a EventBus,
}

impl<'a> SleepFuture<'a> {
    pub fn new(deadline: Duration, event_bus: &'a EventBus) -> Self {
        Self {
            deadline,
            event_bus,
        }
    }
    pub async fn init(self: Pin<&mut Self>) {
        let waker = async_tools::take_waker().await;
        self::timer_push_task(self.deadline, waker.clone());
        self.event_bus.register(Event::all(), waker).unwrap();
    }
}

impl<'a> Future for SleepFuture<'a> {
    type Output = SysResult;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        stack_trace!();
        if self.deadline <= super::get_time() {
            return Poll::Ready(Ok(0));
        } else if self.event_bus.event() != Event::empty() {
            return Poll::Ready(Err(SysError::EINTR));
        }
        Poll::Pending
    }
}
