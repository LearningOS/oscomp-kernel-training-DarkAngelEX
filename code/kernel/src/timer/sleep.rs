use core::{
    cmp::{Ordering, Reverse},
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
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

use super::TimeTicks;

struct TimerCondVar {
    expire_ticks: TimeTicks,
    waker: Waker,
}
impl TimerCondVar {
    pub fn new(expire_ticks: TimeTicks, waker: Waker) -> Self {
        Self {
            expire_ticks,
            waker,
        }
    }
}

impl PartialEq for TimerCondVar {
    fn eq(&self, other: &Self) -> bool {
        self.expire_ticks == other.expire_ticks
    }
}

impl Eq for TimerCondVar {}
impl PartialOrd for TimerCondVar {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.expire_ticks.cmp(&other.expire_ticks))
    }
}
impl Ord for TimerCondVar {
    fn cmp(&self, other: &Self) -> Ordering {
        self.expire_ticks.cmp(&other.expire_ticks)
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
    pub fn ignore(ticks: TimeTicks) -> bool {
        ticks.0 >= usize::MAX as u128
    }
    pub fn push(&mut self, ticks: TimeTicks, waker: Waker) {
        if Self::ignore(ticks) {
            return;
        }
        self.queue.push(Reverse(TimerCondVar::new(ticks, waker)))
    }
    pub fn check_timer(&mut self, current: TimeTicks) {
        stack_trace!();
        while let Some(Reverse(v)) = self.queue.peek() {
            if v.expire_ticks <= current {
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

pub fn timer_push_task(ticks: TimeTicks, waker: Waker) {
    if SleepQueue::ignore(ticks) {
        return;
    }
    SLEEP_QUEUE.lock().as_mut().unwrap().push(ticks, waker);
}

pub fn check_timer() {
    stack_trace!();
    let current = super::get_time_ticks();
    SLEEP_QUEUE.lock().as_mut().unwrap().check_timer(current);
}

pub async fn just_wait(dur: TimeTicks) {
    let mut future = JustWaitFuture::new(dur);
    let mut ptr = Pin::new(&mut future);
    ptr.as_mut().init().await;
    ptr.await
}

struct JustWaitFuture(TimeTicks);
impl JustWaitFuture {
    pub fn new(dur: TimeTicks) -> Self {
        Self(super::get_time_ticks() + dur)
    }
    pub async fn init(self: Pin<&mut Self>) {
        thread::yield_now().await;
        if self.0 <= super::get_time_ticks() {
            return;
        }
        let waker = async_tools::take_waker().await;
        self::timer_push_task(self.0, waker);
    }
}

impl Future for JustWaitFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.0 <= super::get_time_ticks() {
            return Poll::Ready(());
        }
        Poll::Pending
    }
}

pub async fn sleep(deadline: TimeTicks, event_bus: &EventBus) -> SysResult {
    thread::yield_now().await;
    if super::get_time_ticks() >= deadline {
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
    deadline: TimeTicks,
    event_bus: &'a EventBus,
}

impl<'a> SleepFuture<'a> {
    pub fn new(deadline: TimeTicks, event_bus: &'a EventBus) -> Self {
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
        if self.deadline <= super::get_time_ticks() {
            return Poll::Ready(Ok(0));
        } else if self.event_bus.event() != Event::empty() {
            return Poll::Ready(Err(SysError::EINTR));
        }
        Poll::Pending
    }
}
