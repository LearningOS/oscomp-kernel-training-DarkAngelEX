use core::future::Future;

use alloc::collections::VecDeque;
use async_task::{Runnable, Task};

use crate::sync::mutex::SpinNoIrqLock;

pub struct TaskQueue {
    queue: SpinNoIrqLock<Option<VecDeque<Runnable>>>,
}

impl TaskQueue {
    pub const fn new() -> Self {
        Self {
            queue: SpinNoIrqLock::new(None),
        }
    }
    pub fn init(&self) {
        *self.queue.lock(place!()) = Some(VecDeque::new());
    }
    pub fn push(&self, runnable: Runnable) {
        self.queue
            .lock(place!())
            .as_mut()
            .unwrap()
            .push_back(runnable);
    }
    pub fn fetch(&self) -> Option<Runnable> {
        self.queue.lock(place!()).as_mut().unwrap().pop_front()
    }
}

static TASK_QUEUE: TaskQueue = TaskQueue::new();

pub fn init() {
    TASK_QUEUE.init();
}

pub fn spawn<F>(future: F) -> (Runnable, Task<F::Output>)
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    async_task::spawn(future, |runnable| TASK_QUEUE.push(runnable))
}

pub fn run_until_idle() {
    while let Some(task) = TASK_QUEUE.fetch() {
        // println!("fetch task success");
        task.run();
    }
}
