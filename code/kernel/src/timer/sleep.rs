use core::{
    cmp::{Ordering, Reverse},
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
    time::Duration,
};

use ftl_util::{
    async_tools,
    container::max_heap::TraceMaxHeap,
    error::{SysError, SysRet},
    time::Instant,
};

use crate::{
    process::thread,
    sync::{
        even_bus::{self, Event, EventBus},
        mutex::SpinNoIrqLock,
    },
};

struct TimerTracer(usize);

impl Unpin for TimerTracer {}

impl TimerTracer {
    pub fn new() -> Self {
        Self(usize::MAX)
    }
    pub fn in_queue(&self) -> bool {
        self.load() != usize::MAX
    }
    pub fn ptr(&mut self) -> *mut usize {
        &mut self.0
    }
    fn load(&self) -> usize {
        unsafe { core::ptr::read_volatile(&self.0) }
    }
}

impl Drop for TimerTracer {
    fn drop(&mut self) {
        debug_assert_eq!(self.load(), usize::MAX)
    }
}

struct TimerCondVar {
    timeout: Instant,
    waker: Waker,
}

impl TimerCondVar {
    pub fn new(timeout: Instant, waker: Waker) -> Self {
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
    queue: TraceMaxHeap<Reverse<TimerCondVar>>,
}

impl SleepQueue {
    pub fn new() -> Self {
        Self {
            queue: TraceMaxHeap::new(),
        }
    }
    pub fn ignore(timeout: Instant) -> bool {
        timeout.as_secs() >= i64::MAX as u64
    }
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
    pub fn push(&mut self, now: Instant, waker: Waker, tracer: &mut TimerTracer) {
        if Self::ignore(now) {
            return;
        }
        self.queue
            .push((Reverse(TimerCondVar::new(now, waker)), tracer.ptr()))
    }
    /// 返回唤醒的数量
    pub fn check_timer(&mut self, current: Instant) -> usize {
        stack_trace!();
        let mut n = 0;
        while let Some((Reverse(v), _)) = self.queue.peek() {
            if v.timeout <= current {
                self.queue.pop().unwrap().0 .0.waker.wake();
                n += 1;
            } else {
                break;
            }
        }
        n
    }
    pub fn next_instant(&self) -> Option<Instant> {
        self.queue.peek().map(|(a, _)| a.0.timeout)
    }
}

static SLEEP_QUEUE: SpinNoIrqLock<Option<SleepQueue>> = SpinNoIrqLock::new(None);

pub fn sleep_queue_init() {
    *SLEEP_QUEUE.lock() = Some(SleepQueue::new());
}

unsafe fn sq_unlock_run<T>(f: impl FnOnce(&SleepQueue) -> T) -> T {
    f(SLEEP_QUEUE.unsafe_get().as_ref().unwrap_unchecked())
}
fn sq_run<T>(f: impl FnOnce(&mut SleepQueue) -> T) -> T {
    unsafe { f(SLEEP_QUEUE.lock().as_mut().unwrap_unchecked()) }
}

fn push_timer(timeout: Instant, waker: Waker, tracer: &mut TimerTracer) {
    if SleepQueue::ignore(timeout) {
        return;
    }
    sq_run(|q| q.push(timeout, waker, tracer))
}

fn pop_timer(tracer: &mut TimerTracer) {
    if !tracer.in_queue() {
        return;
    }
    sq_run(|q| {
        let idx = tracer.load();
        if idx == usize::MAX {
            return;
        }
        q.queue.remove_idx(idx);
    });
}

/// 返回唤醒的数量
pub fn check_timer() -> usize {
    stack_trace!();
    if unsafe { sq_unlock_run(|q| q.is_empty()) } {
        return 0;
    }
    let current = super::now();
    sq_run(|q| q.check_timer(current))
}
pub fn next_instant() -> Option<Instant> {
    if unsafe { sq_unlock_run(|q| q.is_empty()) } {
        return None;
    }
    sq_run(|q| q.next_instant())
}

struct AlwaysPending;
impl Future for AlwaysPending {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Pending
    }
}

/// 只能被定时器唤醒的future
pub async fn just_wait(dur: Duration) {
    let _ = TimeoutFuture::new(super::now() + dur, AlwaysPending).await;
}

/// 允许线程被定时器或其他的事件唤醒
pub async fn sleep(dur: Duration, event_bus: &EventBus) -> SysRet {
    let deadline = super::now() + dur;
    thread::yield_now().await;
    if super::now() >= deadline {
        return Ok(0);
    }
    if event_bus.event() != Event::empty() {
        return Err(SysError::EINTR);
    }
    let waker = async_tools::take_waker().await;
    match TimeoutFuture::new(
        deadline,
        even_bus::wait_for_event(event_bus, Event::all(), &waker),
    )
    .await
    {
        None => Ok(0),
        Some(_) => Err(SysError::EINTR),
    }
    // let mut future = SleepFuture::new(deadline, event_bus);
    // let mut ptr = Pin::new(&mut future);
    // ptr.as_mut().init().await;
    // ptr.await
}

struct SleepFuture<'a> {
    deadline: Instant,
    event_bus: &'a EventBus,
    tracer: TimerTracer,
}

impl<'a> SleepFuture<'a> {
    pub fn new(deadline: Instant, event_bus: &'a EventBus) -> Self {
        Self {
            deadline,
            event_bus,
            tracer: TimerTracer::new(),
        }
    }
    // pub async fn init(mut self: Pin<&mut Self>) {
    //     let waker = async_tools::take_waker().await;
    //     self::push_timer(self.deadline, waker.clone(), &mut self.tracer);
    //     self.event_bus.register(Event::all(), waker).unwrap();
    // }
}

impl<'a> Future for SleepFuture<'a> {
    type Output = SysRet;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        stack_trace!();
        if self.deadline <= super::now() {
            return Poll::Ready(Ok(0));
        } else if self.event_bus.event() != Event::empty() {
            return Poll::Ready(Err(SysError::EINTR));
        }
        Poll::Pending
    }
}

pub struct TimeoutFuture<T, F: Future<Output = T>> {
    timeout: Instant,
    tracer: Option<TimerTracer>,
    future: F,
}

impl<T, F: Future<Output = T>> Drop for TimeoutFuture<T, F> {
    fn drop(&mut self) {
        self.deatch_timer();
    }
}

impl<T, F: Future<Output = T>> TimeoutFuture<T, F> {
    pub fn new(timeout: Instant, future: F) -> Self {
        Self {
            timeout,
            tracer: None,
            future,
        }
    }
    pub fn inner(self: Pin<&mut Self>) -> Pin<&mut F> {
        unsafe { Pin::new_unchecked(&mut self.get_unchecked_mut().future) }
    }
    fn deatch_timer(&mut self) {
        stack_trace!();
        if let Some(tracer) = &mut self.tracer {
            pop_timer(tracer);
            self.tracer = None;
        }
    }
}

impl<T, F: Future<Output = T>> Future for TimeoutFuture<T, F> {
    type Output = Option<T>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.as_mut().inner().poll(cx) {
            Poll::Ready(r) => {
                unsafe { self.get_unchecked_mut().deatch_timer() }
                return Poll::Ready(Some(r));
            }
            Poll::Pending => (),
        }
        if super::now() >= self.timeout {
            unsafe { self.get_unchecked_mut().deatch_timer() }
            return Poll::Ready(None);
        }
        if self.tracer.is_none() {
            let this = unsafe { self.get_unchecked_mut() };
            this.tracer = Some(TimerTracer::new()); // 不能改变地址!!!
            push_timer(
                this.timeout,
                cx.waker().clone(),
                this.tracer.as_mut().unwrap(),
            );
        }
        Poll::Pending
    }
}
