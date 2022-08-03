use core::task::Waker;

use ftl_util::{
    list::InListNode,
    sync::{spin_mutex::SpinMutex, Spin},
};

bitflags! {
    pub struct PL: u16 {
        const POLLIN    = 1 << 0;
        const POLLPRI   = 1 << 1;
        const POLLOUT   = 1 << 2;
        const POLLRDHUP = 1 << 3;
        const POLLERR   = 1 << 4;
        const POLLNVAL  = 1 << 5;
    }
}

inlist_access!(pub SelectWaiterAccess, SelectNode, node);
/// select 等待器
pub struct SelectNode {
    node: InListNode<Self, SelectWaiterAccess>,
    waker: Option<Waker>,
    flags: PL,
}

impl SelectNode {
    pub const fn new(flags: PL) -> Self {
        Self {
            node: InListNode::new(),
            waker: None,
            flags,
        }
    }
    pub fn init(&mut self) {
        self.node.init();
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
    }
}
