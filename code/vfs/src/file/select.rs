use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

use alloc::{sync::Arc, vec::Vec};
use ftl_util::{
    list::InListNode,
    sync::{spin_mutex::SpinMutex, Spin},
};

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

inlist_access!(pub SelectWaiterAccessIN, SelectNode, node_in);
inlist_access!(pub SelectWaiterAccessPRI, SelectNode, node_pri);
inlist_access!(pub SelectWaiterAccessOUT, SelectNode, node_out);
/// select 等待器
pub struct SelectNode {
    node_in: InListNode<Self, SelectWaiterAccessIN>,
    node_pri: InListNode<Self, SelectWaiterAccessPRI>,
    node_out: InListNode<Self, SelectWaiterAccessOUT>,
    waker: Option<Waker>,
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
    pub fn set_waker(&mut self, waker: Waker) {
        debug_assert!(self.waker.is_none());
        self.waker = Some(waker);
    }
}

/// 在这个文件上等待的集合
pub struct SelectSet {
    head: SpinMutex<SelectNode, Spin>,
}

impl SelectSet {
    pub const fn new() -> Self {
        Self {
            head: SpinMutex::new(SelectNode::new(PL::empty())),
        }
    }
    pub fn push(&self, node: &mut SelectNode) {
        // self.head.lock().
        debug_assert!(node.waker.is_some());
        let mut lk = self.head.lock();
        let events = node.events;
        if events.contains(PL::POLLIN) {
            lk.node_in.push_prev(&mut node.node_in);
        }
        if events.contains(PL::POLLPRI) {
            lk.node_pri.push_prev(&mut node.node_pri);
        }
        if events.contains(PL::POLLOUT) {
            lk.node_out.push_prev(&mut node.node_out);
        }
    }
    pub fn pop(&self, node: &mut SelectNode) {
        let _lk = self.head.lock();
        let events = node.events;
        if events.contains(PL::POLLIN) {
            node.node_in.pop_self();
        }
        if events.contains(PL::POLLPRI) {
            node.node_pri.pop_self();
        }
        if events.contains(PL::POLLOUT) {
            node.node_out.pop_self();
        }
    }

    pub fn wake(&self, events: PL) {
        let lk = self.head.lock();
        if events.contains(PL::POLLIN) {
            lk.node_in
                .next_iter()
                .for_each(|node| node.waker.as_ref().unwrap().wake_by_ref());
        }
        if events.contains(PL::POLLPRI) {
            lk.node_pri
                .next_iter()
                .for_each(|node| node.waker.as_ref().unwrap().wake_by_ref());
        }
        if events.contains(PL::POLLOUT) {
            lk.node_out
                .next_iter()
                .for_each(|node| node.waker.as_ref().unwrap().wake_by_ref());
        }
    }
}

struct SelectFuture {
    nodes: Vec<SelectNode>,
}

impl SelectFuture {
    pub fn new(&self, files: Vec<(Arc<dyn File>, PL)>, waker: Waker) -> Self {
        let mut nodes = Vec::new();
        nodes.resize_with(files.len(), || SelectNode::new(PL::empty()));
        for ((file, events), node) in files.into_iter().zip(nodes.iter_mut()) {
            node.events = events;
            node.revents = file.ppoll();
            node.waker = Some(waker.clone());
            node.init();
            file.push_select_node(node);
        }
        Self { nodes }
    }
}

impl Future for SelectFuture {
    type Output = usize;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        todo!()
    }
}
