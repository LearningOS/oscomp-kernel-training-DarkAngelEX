use core::{
    future::Future,
    marker::PhantomData,
    pin::Pin,
    ptr::NonNull,
    task::{Context, Poll, Waker},
};

use alloc::{sync::Arc, vec::Vec};
use ftl_util::list::InListNode;

use crate::File;

bitflags! {
    pub struct PL: u16 {
        const POLLIN    = 1 << 0;
        const POLLPRI   = 1 << 1;
        const POLLOUT   = 1 << 2;
        const POLLERR   = 1 << 3;
        const POLLHUP   = 1 << 4;
        const POLLNVAL  = 1 << 5;
    }
}

impl PL {
    pub const POLLSUCCESS: Self = Self::POLLIN.union(Self::POLLPRI).union(Self::POLLOUT);
    pub const POLLFAIL: Self = Self::POLLERR.union(Self::POLLHUP).union(Self::POLLNVAL);
}

inlist_access!(pub SelectWaiterAccessIN, SelectNode, node_in);
inlist_access!(pub SelectWaiterAccessPRI, SelectNode, node_pri);
inlist_access!(pub SelectWaiterAccessOUT, SelectNode, node_out);

/// select 等待器
pub struct SelectNode {
    node_in: InListNode<Self, SelectWaiterAccessIN>,
    node_pri: InListNode<Self, SelectWaiterAccessPRI>,
    node_out: InListNode<Self, SelectWaiterAccessOUT>,
    waker: Option<NonNull<Waker>>,
    events: PL,
    revents: PL,
}

impl SelectNode {
    pub const fn new(events: PL) -> Self {
        Self {
            node_in: InListNode::new(),
            node_pri: InListNode::new(),
            node_out: InListNode::new(),
            waker: None,
            events,
            revents: PL::empty(),
        }
    }
    pub fn init(&mut self) {
        self.node_in.init();
        self.node_pri.init();
        self.node_out.init();
    }
    pub fn set_waker(&mut self, waker: NonNull<Waker>) {
        debug_assert!(self.waker.is_none());
        self.waker = Some(waker);
    }
    fn wake(&self) {
        unsafe { self.waker.unwrap().as_ref().wake_by_ref() }
    }
}

/// 在这个文件上等待的集合
pub struct SelectSet {
    head: SelectNode,
}

unsafe impl Send for SelectSet {}
unsafe impl Sync for SelectSet {}

impl SelectSet {
    pub const fn new() -> Self {
        Self {
            head: SelectNode::new(PL::empty()),
        }
    }
    pub fn init(&mut self) {
        self.head.init();
    }
    pub fn push(&mut self, node: &mut SelectNode) {
        // self.head.lock().
        debug_assert!(node.waker.is_some());
        let events = node.events;
        if events.contains(PL::POLLIN) {
            self.head.node_in.push_prev(&mut node.node_in);
        }
        if events.contains(PL::POLLPRI) {
            self.head.node_pri.push_prev(&mut node.node_pri);
        }
        if events.contains(PL::POLLOUT) {
            self.head.node_out.push_prev(&mut node.node_out);
        }
    }
    pub fn pop(&mut self, node: &mut SelectNode) {
        let events = node.events;
        if events.intersects(PL::POLLIN) {
            node.node_in.pop_self();
        }
        if events.intersects(PL::POLLPRI) {
            node.node_pri.pop_self();
        }
        if events.intersects(PL::POLLOUT) {
            node.node_out.pop_self();
        }
    }
    pub fn wake(&self, events: PL) {
        stack_trace!();
        if events.intersects(PL::POLLIN | PL::POLLFAIL) {
            self.head.node_in.next_iter().for_each(|node| node.wake());
        }
        if events.intersects(PL::POLLPRI | PL::POLLFAIL) {
            self.head.node_pri.next_iter().for_each(|node| node.wake());
        }
        if events.intersects(PL::POLLOUT | PL::POLLFAIL) {
            self.head.node_out.next_iter().for_each(|node| node.wake());
        }
    }
}

pub struct SelectFuture<'a> {
    nodes: Vec<(usize, Arc<dyn File>, SelectNode)>,
    _mark: PhantomData<&'a mut Waker>,
}

impl Drop for SelectFuture<'_> {
    fn drop(&mut self) {
        self.release();
    }
}

unsafe impl Send for SelectFuture<'_> {}
unsafe impl Sync for SelectFuture<'_> {}

impl<'a> SelectFuture<'a> {
    pub fn new(files: Vec<(usize, Arc<dyn File>, PL)>, waker: &'a mut Waker) -> Self {
        debug_assert!(!files.is_empty());
        let mut nodes: Vec<_> = files
            .into_iter()
            .map(|(fd, f, pl)| (fd, f, SelectNode::new(pl | PL::POLLFAIL)))
            .collect();
        for (_, file, node) in nodes.iter_mut() {
            node.init();
            node.set_waker(NonNull::new(waker).unwrap());
            file.push_select_node(node);
        }
        Self {
            nodes,
            _mark: PhantomData,
        }
    }
    fn release(&mut self) {
        for (_, file, node) in self.nodes.iter_mut() {
            file.pop_select_node(node);
        }
        self.nodes.clear();
    }
    fn detach(self: Pin<&mut Self>) -> Vec<(usize, PL)> {
        let this = unsafe { self.get_unchecked_mut() };
        let mut ret = Vec::new();
        for (fd, file, node) in this.nodes.iter_mut() {
            file.pop_select_node(node);
            if !node.revents.is_empty() {
                ret.push((*fd, node.revents));
            }
        }
        this.nodes.clear();
        ret
    }
}

impl Future for SelectFuture<'_> {
    type Output = Vec<(usize, PL)>;

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { &mut self.as_mut().get_unchecked_mut() };
        let nodes = &mut this.nodes;
        let mut run = false;
        for (_, file, node) in nodes.iter_mut() {
            node.revents = file.ppoll() & node.events;
            run |= !node.revents.is_empty();
        }
        if !run {
            return Poll::Pending;
        }
        Poll::Ready(self.detach())
    }
}
