//! Modify from rCore

use super::mutex::SpinNoIrqLock as Mutex;
use core::{
    future::Future,
    pin::Pin,
    sync::atomic::{self, Ordering},
    task::{Context, Poll, Waker},
};

use crate::process::{AliveProcess, Dead};
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use ftl_util::{async_tools::WakerPtr, error::SysError, list::ListNode};

bitflags! {
    pub struct Event: u32 {
        const EMPTY                  = 0;
        /// File
        const READABLE               = 1 << 0;
        const WRITABLE               = 1 << 1;
        const ERROR                  = 1 << 2;
        const CLOSED                 = 1 << 3;

        /// Process
        const PROCESS_QUIT           = 1 << 10;
        const CHILD_PROCESS_QUIT     = 1 << 11;
        const RECEIVE_SIGNAL         = 1 << 12;
        const REMOTE_RUN             = 1 << 13;

        /// Semaphore
        const SEMAPHORE_REMOVED      = 1 << 20;
        const SEMAPHORE_CAN_ACQUIRE  = 1 << 21;
    }
}

#[derive(Debug)]
pub struct EvenBusClosed;

impl From<EvenBusClosed> for () {
    fn from(_: EvenBusClosed) -> Self {}
}
impl From<EvenBusClosed> for Dead {
    fn from(_: EvenBusClosed) -> Self {
        Dead
    }
}
impl From<EvenBusClosed> for SysError {
    fn from(_: EvenBusClosed) -> Self {
        Dead.into()
    }
}

pub struct EventBus(Mutex<EventBusInner>);

struct BusNode {
    mask: Event,
    suspend: Event,
    waker: WakerPtr,
}

impl BusNode {
    fn new(mask: Event, waker: WakerPtr) -> Self {
        Self {
            mask,
            suspend: Event::empty(),
            waker,
        }
    }
    fn wake(&mut self) {
        debug_assert!(self.waker != WakerPtr::dangling());
        self.waker.wake();
    }
}

pub struct EventBusInner {
    closed: bool,
    pub event: Event,
    suspend_event: Event,
    wakers: ListNode<BusNode>,
    remote: Vec<Box<dyn FnOnce(&mut AliveProcess) + Send + 'static>>,
}

impl Drop for EventBusInner {
    fn drop(&mut self) {
        stack_trace!();
        debug_assert!(
            !self.should_suspend(),
            "impossible status in event_bus drop!"
        );
        assert!(self.wakers.is_empty());
    }
}

impl EventBus {
    pub fn new() -> Arc<Self> {
        let bus = Arc::new(Self(Mutex::new(EventBusInner {
            closed: false,
            event: Event::empty(),
            suspend_event: Event::empty(),
            wakers: ListNode::new(BusNode::new(Event::empty(), WakerPtr::dangling())),
            remote: Vec::new(),
        })));
        unsafe { bus.0.unsafe_get_mut().wakers.init() }
        bus
    }
    fn close_check(&self) -> Result<(), EvenBusClosed> {
        if !unsafe { self.0.unsafe_get().closed } {
            Ok(())
        } else {
            Err(EvenBusClosed)
        }
    }
    pub fn close(&self) {
        self.0.lock().close();
    }
    pub fn event(&self) -> Event {
        unsafe { self.0.unsafe_get().event }
    }
    fn clear_then_set_unchanged(&self, reset: Event, set: Event) -> bool {
        let cur = self.event();
        cur == (cur & !reset) | set
    }
    pub fn set(&self, set: Event) -> Result<(), EvenBusClosed> {
        self.close_check()?;
        if self.clear_then_set_unchanged(Event::EMPTY, set) {
            return Ok(());
        }
        self.0.lock().set(set)
    }
    pub fn clear(&self, reset: Event) -> Result<(), EvenBusClosed> {
        self.close_check()?;
        if self.clear_then_set_unchanged(reset, Event::EMPTY) {
            return Ok(());
        }
        self.0.lock().clear(reset)
    }
    pub fn clear_then_set(&self, reset: Event, set: Event) -> Result<(), EvenBusClosed> {
        self.close_check()?;
        if self.clear_then_set_unchanged(reset, set) {
            return Ok(());
        }
        self.0.lock().clear_then_set(reset, set)
    }
    fn register(&self, node: &mut ListNode<BusNode>) -> Result<(), EvenBusClosed> {
        self.close_check()?;
        self.0.lock().register(node)
    }
    fn remove(&self, node: &mut ListNode<BusNode>) {
        if node.is_empty_race() {
            return;
        }
        self.0.lock().remove(node)
    }
    // pub fn remote_run(&self,)
}

impl EventBusInner {
    pub fn close(&mut self) {
        stack_trace!();
        // assert!(!self.closed, "event_bus double closed");
        assert!(!self.should_suspend(), "impossible status in close!");
        self.closed = true;
        self.wakers.pop_all(|node| node.wake());
    }
    pub fn set(&mut self, set: Event) -> Result<(), EvenBusClosed> {
        self.clear_then_set(Event::empty(), set)
    }
    pub fn clear(&mut self, reset: Event) -> Result<(), EvenBusClosed> {
        self.clear_then_set(reset, Event::empty())
    }
    pub fn clear_then_set(&mut self, reset: Event, set: Event) -> Result<(), EvenBusClosed> {
        if self.closed {
            return Err(EvenBusClosed);
        }
        self.event.remove(reset);
        self.event.insert(set);
        self.suspend();
        Ok(())
    }
    fn should_suspend(&self) -> bool {
        self.event.intersects(self.suspend_event)
    }
    fn suspend(&mut self) {
        if !self.should_suspend() {
            return;
        }
        let event_cur = self.event;
        let mut suspend_event = Event::empty();
        self.wakers.pop_when_race(
            |node| {
                if (node.mask & event_cur).is_empty() {
                    suspend_event.insert(node.mask);
                    false
                } else {
                    true
                }
            },
            // release 在 pop_self之前运行, 而如果析构函数没有观测到 pop_self 则会获取锁,
            // 因此这里的wake()函数一定是有效的
            |node| {
                node.suspend = event_cur;
                atomic::fence(Ordering::Release);
                node.wake();
            },
            |_| (),
        );

        self.suspend_event = suspend_event;
    }
    fn register(&mut self, node: &mut ListNode<BusNode>) -> Result<(), EvenBusClosed> {
        stack_trace!();
        debug_assert!(node.inited());
        if self.closed {
            return Err(EvenBusClosed);
        }
        if self.event.intersects(node.data().mask) {
            node.data_mut().suspend = self.event;
            node.data_mut().wake();
        } else {
            self.suspend_event.insert(node.data().mask);
            self.wakers.push_prev(node);
        }
        Ok(())
    }
    fn remove(&mut self, node: &mut ListNode<BusNode>) {
        if node.is_empty_race() {
            return;
        }
        node.pop_self();
    }
}

pub async fn wait_for_event(
    bus: &EventBus,
    mask: Event,
    waker: &Waker,
) -> Result<Event, EvenBusClosed> {
    EventBusFuture {
        bus,
        mask,
        node: None,
        waker,
    }
    .await
}

struct EventBusFuture<'a> {
    bus: &'a EventBus,
    mask: Event,
    node: Option<ListNode<BusNode>>,
    waker: &'a Waker,
}

impl Drop for EventBusFuture<'_> {
    fn drop(&mut self) {
        if let Some(node) = self.node.as_mut() {
            self.bus.remove(node);
        }
    }
}

impl Future for EventBusFuture<'_> {
    type Output = Result<Event, EvenBusClosed>;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Self::Output> {
        stack_trace!();
        if self.node.is_none() {
            if self.bus.close_check().is_err() {
                return Poll::Ready(Err(EvenBusClosed));
            }
            let event = self.bus.event();
            if event.intersects(self.mask) {
                return Poll::Ready(Ok(event));
            }
            let mut lock = self.bus.0.lock();
            let this = unsafe { self.get_unchecked_mut() };
            this.node = Some(ListNode::new(BusNode {
                mask: this.mask,
                suspend: Event::empty(),
                waker: WakerPtr::new(this.waker),
            }));
            let node = this.node.as_mut().unwrap();
            node.init();
            match lock.register(node) {
                Ok(()) => return Poll::Pending,
                Err(_e) => return Poll::Ready(Err(EvenBusClosed)),
            }
        }
        let this = unsafe { self.get_unchecked_mut() };
        let node = this.node.as_mut().unwrap();
        let suspend = node.data().suspend;
        if !suspend.is_empty() {
            debug_assert!(suspend.intersects(node.data().mask));
            this.bus.remove(node);
            this.node = None;
            return Poll::Ready(Ok(suspend));
        }
        if this.bus.close_check().is_err() {
            this.bus.remove(node);
            this.node = None;
            return Poll::Ready(Err(EvenBusClosed));
        }
        Poll::Pending
    }
}

pub struct EventFuture<'a> {
    bus: &'a EventBus,
    mask: Event,
}

impl<'a> EventFuture<'a> {
    pub fn new(bus: &'a EventBus, mask: Event) -> Self {
        Self { bus, mask }
    }
}

impl Future for EventFuture<'_> {
    type Output = Event;
    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        todo!()
    }
}
