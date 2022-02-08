use crate::sync::mutex::SpinLock;

use super::{
    pid::Pid,
    tid::{Tid, TidAllocator},
};

pub struct ThreadGroup {
    tid_allocator: SpinLock<TidAllocator>,
}

impl ThreadGroup {
    pub fn new(pid: Pid) -> Self {
        Self {
            tid_allocator: SpinLock::new(TidAllocator::new(pid.get_usize())),
        }
    }
    pub fn alloc(&self) -> Tid {
        self.tid_allocator.lock().alloc()
    }
    pub unsafe fn dealloc(&self, tid: Tid) {
        self.tid_allocator.lock().dealloc(tid);
    }
}
