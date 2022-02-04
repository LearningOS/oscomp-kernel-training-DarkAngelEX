use alloc::vec::Vec;

use crate::sync::mutex::SpinLock;

struct PidAllocator {
    current: usize,
    recycled: Vec<usize>,
}

impl PidAllocator {
    pub const fn new() -> Self {
        PidAllocator {
            current: 0,
            recycled: Vec::new(),
        }
    }
    pub fn alloc(&mut self) -> PidHandle {
        if let Some(pid) = self.recycled.pop() {
            PidHandle(pid)
        } else {
            let cur = self.current;
            self.current += 1;
            PidHandle(cur)
        }
    }
    fn dealloc(&mut self, pid: usize) {
        // assert!(pid < self.current);
        // assert!(
        //     !self.recycled.iter().any(|ppid| *ppid == pid),
        //     "pid {} has been deallocated!",
        //     pid
        // );
        self.recycled.push(pid);
    }
}

pub struct PidHandle(pub usize);

impl Drop for PidHandle {
    fn drop(&mut self) {
        //println!("drop pid {}", self.0);
        PID_ALLOCATOR.lock().dealloc(self.0);
    }
}

static PID_ALLOCATOR: SpinLock<PidAllocator> = SpinLock::new(PidAllocator::new());
