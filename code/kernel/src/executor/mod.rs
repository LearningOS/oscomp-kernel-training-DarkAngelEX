use core::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use alloc::{boxed::Box, collections::VecDeque};
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

/// 生成一个完全没有状态切换的内核线程
///
/// 内核线程使用全局页表, 永远不要在内核线程中访问用户态数据!
pub fn kernel_spawn<F: Future<Output = ()> + Send + 'static>(kernel_thread: F) {
    let (runnable, task) = spawn(kernel_thread);
    runnable.schedule();
    task.detach();
}


struct BlockOnFuture<F: Future> {
    future: Pin<Box<F>>,
}
impl<F: Future> BlockOnFuture<F> {
    pub fn new(future: F) -> Self {
        Self {
            future: Box::pin(future),
        }
    }
}
impl<F: Future> Future for BlockOnFuture<F> {
    type Output = F::Output;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            let ans = self.future.as_mut().poll(cx);
            if let Poll::Ready(ret) = ans {
                break Poll::Ready(ret);
            }
        }
    }
}
/// loop forever until future return Poll::Ready
pub fn block_on<F>(future: F) -> F::Output
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let mut ans = None;
    let queue = RefCell::new(None);
    let (r, _t) = unsafe {
        async_task::spawn_unchecked(
            BlockOnFuture::new(async {
                let x = future.await;
                ans = Some(x);
            }),
            |r| {
                queue.borrow_mut().replace(r).is_some().then(|| panic!());
            },
        )
    };
    r.schedule();
    while let Some(r) = queue.borrow_mut().take() {
        r.run();
    }
    ans.unwrap()
}

pub fn run_until_idle() {
    while let Some(task) = TASK_QUEUE.fetch() {
        stack_trace!();
        task.run();
    }
}
