use core::cmp::Ordering;

use alloc::{borrow::ToOwned, collections::BinaryHeap, sync::Arc, vec::Vec};

use crate::{scheduler, sync::mutex::SpinLock, task::TaskControlBlock};

use super::{get_time_ticks, TimeTicks};

struct TimerCondVar {
    expire_ticks: TimeTicks,
    task: Arc<TaskControlBlock>,
}
impl TimerCondVar {
    pub fn new(expire_ticks: TimeTicks, task: Arc<TaskControlBlock>) -> Self {
        Self { expire_ticks, task }
    }
    pub fn take_task(self) -> Arc<TaskControlBlock> {
        self.task
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
    pub fn push(&mut self, ticks: TimeTicks, task: Arc<TaskControlBlock>) {
        self.queue.push(TimerCondVar::new(ticks, task))
    }
    pub fn check_timer(&mut self) {
        let current = get_time_ticks();
        let mut ready = Vec::new();
        while let Some(v) = self.queue.peek() {
            if v.expire_ticks <= current {
                ready.push(self.queue.pop().unwrap().take_task());
            } else {
                break;
            }
        }
        ready
            .iter()
            .for_each(|tcb| unsafe { tcb.become_running_by_scheduler() });
        scheduler::add_task_group(ready.into_iter());
    }
}

static SLEEP_QUEUE: SpinLock<Option<SleepQueue>> = SpinLock::new(None);

pub fn sleep_queue_init() {
    *SLEEP_QUEUE.lock(place!()) = Some(SleepQueue::new());
}

pub fn timer_push_task(ticks: TimeTicks, task: Arc<TaskControlBlock>) {
    SLEEP_QUEUE.lock(place!()).as_mut().unwrap().push(ticks, task);
}

pub fn check_timer() {
    SLEEP_QUEUE.lock(place!()).as_mut().unwrap().check_timer()
}
