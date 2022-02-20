//! Modify from rCore

use super::mutex::SpinNoIrqLock as Mutex;
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

use alloc::{sync::Arc, vec::Vec};

bitflags! {
    #[derive(Default)]
    pub struct Event: u32 {
        /// File
        const READABLE                      = 1 << 0;
        const WRITABLE                      = 1 << 1;
        const ERROR                         = 1 << 2;
        const CLOSED                        = 1 << 3;

        /// Process
        const PROCESS_QUIT                  = 1 << 10;
        const CHILD_PROCESS_QUIT            = 1 << 11;
        const RECEIVE_SIGNAL                = 1 << 12;

        /// Semaphore
        const SEMAPHORE_REMOVED             = 1 << 20;
        const SEMAPHORE_CAN_ACQUIRE         = 1 << 21;
    }
}

#[derive(Debug)]
pub struct EvenBusClose;

impl From<EvenBusClose> for () {
    fn from(_: EvenBusClose) -> Self {
        ()
    }
}

#[derive(Default)]
pub struct EventBus {
    closed: bool,
    event: Event,
    suspend_event: Event,
    wakers: Vec<(Event, Waker)>,
}

impl Drop for EventBus {
    fn drop(&mut self) {
        debug_check!(!self.should_suspend(), "impossible status in event_bus drop!");
    }
}

impl EventBus {
    pub fn new() -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self::default()))
    }
    pub fn close(&mut self) {
        assert!(!self.closed, "event_bus double closed");
        assert!(!self.should_suspend(), "impossible status in close!");
        self.closed = true;
        self.wakers.clear();
    }
    pub fn set(&mut self, set: Event) -> Result<(), EvenBusClose> {
        self.clear_then_set(Event::empty(), set)
    }
    pub fn clear(&mut self, reset: Event) -> Result<(), EvenBusClose> {
        self.clear_then_set(reset, Event::empty())
    }
    pub fn clear_then_set(&mut self, reset: Event, set: Event) -> Result<(), EvenBusClose> {
        if self.closed {
            return Err(EvenBusClose);
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
    pub fn register(&mut self, event: Event, waker: Waker) -> Result<(), EvenBusClose> {
        if self.closed {
            return Err(EvenBusClose);
        }
        self.suspend_event.insert(event);
        self.wakers.push((event, waker));
        Ok(())
    }
}

pub fn wait_for_event(
    bus: Arc<Mutex<EventBus>>,
    mask: Event,
) -> impl Future<Output = Result<Event, EvenBusClose>> {
    EventBusFuture { bus, mask }
}

struct EventBusFuture {
    bus: Arc<Mutex<EventBus>>,
    mask: Event,
}

impl Future for EventBusFuture {
    type Output = Result<Event, EvenBusClose>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let mut lock = self.bus.lock(place!());
        if lock.event.intersects(self.mask) {
            return Poll::Ready(Ok(lock.event));
        }
        lock.register(self.mask, cx.waker().clone())?;
        Poll::Pending
    }
}
