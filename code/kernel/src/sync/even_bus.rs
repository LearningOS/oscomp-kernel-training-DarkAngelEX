//! Modify from rCore

use super::mutex::SpinNoIrqLock as Mutex;
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

use crate::{
    process::{AliveProcess, Dead},
    tools::container::{never_clone_linked_list::NeverCloneLinkedList, Stack},
};
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use ftl_util::error::SysError;

bitflags! {
    #[derive(Default)]
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

#[derive(Default)]
pub struct EventBus(Mutex<EventBusInner>);

#[derive(Default)]
pub struct EventBusInner {
    closed: bool,
    pub event: Event,
    suspend_event: Event,
    wakers: NeverCloneLinkedList<(Event, Waker)>,
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
        Arc::new(Self(Mutex::new(EventBusInner::default())))
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
    pub fn register(&self, event: Event, waker: Waker) -> Result<(), EvenBusClosed> {
        self.close_check()?;
        self.0.lock().register(event, waker)
    }
    // pub fn remote_run(&self,)
}

impl EventBusInner {
    pub fn close(&mut self) {
        stack_trace!();
        // assert!(!self.closed, "event_bus double closed");
        assert!(!self.should_suspend(), "impossible status in close!");
        self.closed = true;
        while let Some((_e, waker)) = self.wakers.pop() {
            waker.wake();
        }
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
        self.wakers.retain(|(event, waker)| {
            if *event & event_cur != Event::empty() {
                waker.wake_by_ref();
                false
            } else {
                suspend_event.insert(*event);
                true
            }
        });
        self.suspend_event = suspend_event;
    }
    pub fn register(&mut self, event: Event, waker: Waker) -> Result<(), EvenBusClosed> {
        if self.closed {
            return Err(EvenBusClosed);
        }
        if self.event.intersects(event) {
            waker.wake();
        } else {
            self.suspend_event.insert(event);
            self.wakers.push((event, waker));
        }
        Ok(())
    }
}

pub async fn wait_for_event(bus: &EventBus, mask: Event) -> Result<Event, EvenBusClosed> {
    EventBusFuture { bus, mask }.await
}

struct EventBusFuture<'a> {
    bus: &'a EventBus,
    mask: Event,
}

impl Future for EventBusFuture<'_> {
    type Output = Result<Event, EvenBusClosed>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let mut lock = self.bus.0.lock();
        if lock.event.intersects(self.mask) {
            return Poll::Ready(Ok(lock.event));
        }
        lock.register(self.mask, cx.waker().clone())?;
        Poll::Pending
    }
}
