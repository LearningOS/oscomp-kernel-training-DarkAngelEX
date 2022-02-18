use crate::sync::mutex::SpinNoIrqLock;

use super::{
    pid::Pid,
    tid::{Tid, TidAllocator},
};

#[derive(Debug)]
pub struct LockedThreadGroup {
    tid_allocator: SpinNoIrqLock<TidAllocator>,
}

impl LockedThreadGroup {
    pub fn new(pid: Pid) -> Self {
        Self {
            tid_allocator: SpinNoIrqLock::new(TidAllocator::new(pid.into_usize())),
        }
    }
    pub fn alloc(&self) -> Tid {
        self.tid_allocator.lock(place!()).alloc()
    }
    pub unsafe fn dealloc(&self, tid: Tid) {
        self.tid_allocator.lock(place!()).dealloc(tid);
    }
}
