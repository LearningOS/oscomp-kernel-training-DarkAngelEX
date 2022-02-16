use alloc::{boxed::Box, collections::LinkedList, sync::Weak};

use crate::{
    sync::mutex::SpinLock,
    task::{children::ChildrenSet, Pid, TaskControlBlock},
};

pub enum Message {
    ChildBecomeZombie(Pid),               // send from child to parent
    MoveChildren(Box<ChildrenSet>),       // send children to initproc
    ChangeParent(Weak<TaskControlBlock>), // let child's parent become initproc
}

struct MessageQueue {
    queue: LinkedList<Message>,
}
impl Drop for MessageQueue {
    fn drop(&mut self) {
        assert!(self.is_empty());
    }
}
impl MessageQueue {
    fn new() -> Self {
        Self {
            queue: LinkedList::new(),
        }
    }
    fn push(&mut self, msg: Message) {
        self.queue.push_front(msg);
    }
    fn pop(&mut self) -> Option<Message> {
        self.queue.pop_back()
    }
    fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

pub struct MessageReceive {
    queue: SpinLock<Option<Box<MessageQueue>>>,
}
impl Drop for MessageReceive {
    fn drop(&mut self) {
        debug_check!(self.is_close_or_empty());
    }
}

pub struct MessageProcess {
    queue: Box<MessageQueue>,
}
impl Drop for MessageProcess {
    fn drop(&mut self) {
        debug_check!(self.is_empty());
    }
}

impl MessageReceive {
    pub fn new() -> Self {
        Self {
            queue: SpinLock::new(Some(Box::new(MessageQueue::new()))),
        }
    }
    /// return Err if have close.
    pub fn receive(&self, msg: Message) -> Result<(), Message> {
        match &mut *self.queue.lock(place!()) {
            Some(q) => {
                q.push(msg);
                Ok(())
            }
            None => Err(msg),
        }
    }
    /// will lock queue
    pub fn is_close_or_empty(&self) -> bool {
        match self.queue.lock(place!()).as_ref() {
            Some(q) => q.is_empty(),
            None => true,
        }
    }
    /// will lock queue
    pub fn is_close(&self) -> bool {
        self.queue.lock(place!()).is_none()
    }
}

impl MessageProcess {
    pub fn new() -> Self {
        Self {
            queue: Box::new(MessageQueue::new()),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
    pub fn take_from(&mut self, src: &MessageReceive) {
        assert!(self.queue.is_empty());
        core::mem::swap(
            &mut self.queue,
            &mut src.queue.lock(place!()).as_mut().unwrap(),
        );
    }
    pub fn take_and_close_from(&mut self, src: &MessageReceive) {
        assert!(self.queue.is_empty());
        let mut lock = src.queue.lock(place!());
        core::mem::swap(&mut self.queue, &mut lock.as_mut().unwrap());
        *lock = None;
    }
    pub fn pop(&mut self) -> Option<Message> {
        self.queue.pop()
    }
}
