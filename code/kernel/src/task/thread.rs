use crate::sync::mutex::SpinLock;

use super::{
    pid::Pid,
    tid::{Tid, TidAllocator},
};

#[derive(Debug)]
pub struct LockedThreadGroup {
    tid_allocator: SpinLock<TidAllocator>,
}

impl LockedThreadGroup {
    pub fn new(pid: Pid) -> Self {
        Self {
            tid_allocator: SpinLock::new(TidAllocator::new(pid.into_usize())),
        }
    }
    pub fn alloc(&self) -> Tid {
        self.tid_allocator.lock().alloc()
    }
    pub unsafe fn dealloc(&self, tid: Tid) {
        self.tid_allocator.lock().dealloc(tid);
    }
}
