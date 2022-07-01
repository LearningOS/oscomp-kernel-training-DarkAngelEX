use crate::tools::{
    allocator::{from_usize_allocator::FromUsizeAllocator, Own},
    container::never_clone_linked_list::NeverCloneLinkedList,
    Wrapper,
};

use super::Tid;

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct Pid(pub usize);

from_usize_impl!(Pid);

struct PidWrapper;
impl Wrapper<Pid> for PidWrapper {
    type Output = PidHandle;
    fn wrapper(a: Pid) -> PidHandle {
        PidHandle(a)
    }
}

type PidAllocator = FromUsizeAllocator<Pid, PidWrapper, NeverCloneLinkedList<usize>>;

#[derive(Debug)]
pub struct PidHandle(Pid);

impl Own<Pid> for PidHandle {}

impl PidHandle {
    pub fn pid(&self) -> Pid {
        self.0
    }
    pub fn get_usize(&self) -> usize {
        self.pid().0
    }
}

impl Drop for PidHandle {
    fn drop(&mut self) {
        //println!("drop pid {}", self.0);
        unsafe { super::tid::pidhandle_dealloc_impl(self.0) }
    }
}

pub(super) unsafe fn pid_alloc_by_tid(tid: Tid) -> PidHandle {
    PidHandle(Pid(tid.0))
}
