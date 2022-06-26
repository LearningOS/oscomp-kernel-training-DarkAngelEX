use core::{
    cmp::Ordering,
    future::Future,
    task::{Context, Poll, Waker},
};

use alloc::{collections::BinaryHeap, sync::Arc};
use ftl_util::error::SysError;

use crate::{
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
    queue: BinaryHeap<TimerCondVar>,
}
impl SleepQueue {
    pub fn new() -> Self {
        Self {
            queue: BinaryHeap::new(),
        }
    }
    pub fn push(&mut self, ticks: TimeTicks, waker: Waker) {
        self.queue.push(TimerCondVar::new(ticks, waker))
    }
    pub fn check_timer(&mut self, current: TimeTicks) {
        stack_trace!();
        while let Some(v) = self.queue.peek() {
            if v.expire_ticks <= current {
                self.queue.pop().unwrap().waker.wake();
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
    SLEEP_QUEUE.lock().as_mut().unwrap().push(ticks, waker);
}

pub fn check_timer() {
    stack_trace!();
    let current = super::get_time_ticks();
    SLEEP_QUEUE.lock().as_mut().unwrap().check_timer(current);
}

pub struct JustWaitFuture(TimeTicks, bool);
impl JustWaitFuture {
    pub fn new(dur: TimeTicks) -> Self {
        Self(super::get_time_ticks() + dur, false)
    }
}

impl Future for JustWaitFuture {
    type Output = ();

    fn poll(mut self: core::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        stack_trace!();
        if super::get_time_ticks() >= self.0 {
            return Poll::Ready(());
        }
        if !self.1 {
            self::timer_push_task(self.0, cx.waker().clone());
            self.1 = true;
        }
        Poll::Pending
    }
}

pub struct SleepFuture {
    deadline: TimeTicks,
    event_bus: Arc<EventBus>,
    inited: bool,
}

impl SleepFuture {
    pub fn new(deadline: TimeTicks, event_bus: Arc<EventBus>) -> Self {
        Self {
            deadline,
            event_bus,
            inited: false,
        }
    }
}

impl Future for SleepFuture {
    type Output = SysResult;

    fn poll(mut self: core::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        stack_trace!();
        if super::get_time_ticks() >= self.deadline {
            return Poll::Ready(Ok(0));
        } else if self.event_bus.event() != Event::empty() {
            return Poll::Ready(Err(SysError::EINTR));
        }
        if !self.inited {
            self::timer_push_task(self.deadline, cx.waker().clone());
            self.inited = true;
        }
        match self.event_bus.register(Event::all(), cx.waker().clone()) {
            Err(_e) => Poll::Ready(Err(SysError::ESRCH)),
            Ok(()) => Poll::Pending,
        }
    }
}
