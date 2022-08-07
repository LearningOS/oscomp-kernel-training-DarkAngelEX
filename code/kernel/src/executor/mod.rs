use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use alloc::collections::VecDeque;
use async_task::{Runnable, Task};

use crate::{
    local::{self, always_local::AlwaysLocal},
    sync::mutex::SpinNoIrqLock,
};

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
        *self.queue.lock() = Some(VecDeque::new());
    }
    pub fn push(&self, runnable: Runnable) {
        self.queue.lock().as_mut().unwrap().push_back(runnable);
    }
    pub fn fetch(&self) -> Option<Runnable> {
        self.queue.lock().as_mut().unwrap().pop_front()
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
    async_task::spawn(future, |runnable| {
        TASK_QUEUE.push(runnable);
    })
}

/// 生成一个不切换页表的内核线程
///
/// 内核线程使用全局页表, 永远不要在内核线程中访问用户态数据!
pub fn kernel_spawn<F: Future<Output = ()> + Send + 'static>(kernel_thread: F) {
    let (runnable, task) = spawn(KernelTaskFuture::new(kernel_thread));
    runnable.schedule();
    task.detach();
}

struct KernelTaskFuture<F: Future<Output = ()> + Send + 'static> {
    always_local: AlwaysLocal,
    task: F,
}
impl<F: Future<Output = ()> + Send + 'static> KernelTaskFuture<F> {
    pub fn new(task: F) -> Self {
        Self {
            always_local: AlwaysLocal::new(),
            task,
        }
    }
}

impl<F: Future<Output = ()> + Send + 'static> Future for KernelTaskFuture<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        unsafe {
            let local = local::hart_local();
            let this = self.get_unchecked_mut();
            local.local_rcu.critical_start();
            local.handle();
            local.switch_kernel_task(&mut this.always_local);
            let r = Pin::new_unchecked(&mut this.task).poll(cx);
            local.switch_kernel_task(&mut this.always_local);
            local.local_rcu.critical_end_tick();
            local.handle();
            r
        }
    }
}

/// 返回执行了多少个future
pub fn run_until_idle() -> usize {
    let mut n = 0;
    while let Some(task) = TASK_QUEUE.fetch() {
        stack_trace!();
        task.run();
        n += 1;
    }
    n
}
