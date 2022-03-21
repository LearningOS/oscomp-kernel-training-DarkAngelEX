use core::{cmp::Ordering, task::Waker};

use alloc::collections::BinaryHeap;

use crate::sync::mutex::SpinNoIrqLock;

use super::{get_time_ticks, TimeTicks};

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
    pub fn check_timer(&mut self) {
        stack_trace!();
        let current = get_time_ticks();
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
    *SLEEP_QUEUE.lock(place!()) = Some(SleepQueue::new());
}

pub fn timer_push_task(ticks: TimeTicks, waker: Waker) {
    SLEEP_QUEUE
        .lock(place!())
        .as_mut()
        .unwrap()
        .push(ticks, waker);
}

pub fn check_timer() {
    stack_trace!();
    SLEEP_QUEUE.lock(place!()).as_mut().unwrap().check_timer();
}
