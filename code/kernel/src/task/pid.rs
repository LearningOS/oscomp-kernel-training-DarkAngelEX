use crate::{
    from_usize_impl, sync::mutex::SpinLock,
    tools::allocator::{from_usize_allocator::FromUsizeAllocator, Own},
};

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct Pid(usize);

impl Pid {
    pub fn into_usize(&self) -> usize {
        self.0
    }
}

from_usize_impl!(Pid);

type PidAllocator = FromUsizeAllocator<Pid, PidHandle>;

#[derive(Debug)]
pub struct PidHandle(Pid);

impl Own<Pid> for PidHandle {}

impl PidHandle {
    pub fn pid(&self) -> Pid {
        self.0
    }
    pub fn get_usize(&self) -> usize {
        self.pid().into_usize()
    }
}

impl From<Pid> for PidHandle {
    fn from(pid: Pid) -> Self {
        Self(pid)
    }
}

impl Drop for PidHandle {
    fn drop(&mut self) {
        unsafe {
            //println!("drop pid {}", self.0);
            PID_ALLOCATOR.lock().dealloc(self.pid());
        }
    }
}

static PID_ALLOCATOR: SpinLock<PidAllocator> = SpinLock::new(PidAllocator::new(0));

pub fn pid_alloc() -> PidHandle {
    PID_ALLOCATOR.lock().alloc()
}
