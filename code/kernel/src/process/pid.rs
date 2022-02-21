use crate::{
    from_usize_impl,
    sync::mutex::SpinLock,
    tools::{
        allocator::{from_usize_allocator::FromUsizeAllocator, Own},
        Wrapper,
    },
};

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct Pid(usize);

impl Pid {
    pub fn into_usize(&self) -> usize {
        self.0
    }
}

from_usize_impl!(Pid);

struct PidWrapper;
impl Wrapper<Pid> for PidWrapper {
    type Output = PidHandle;
    fn wrapper(a: Pid) -> PidHandle {
        PidHandle(a)
    }
}

type PidAllocator = FromUsizeAllocator<Pid, PidWrapper>;

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

impl Drop for PidHandle {
    fn drop(&mut self) {
        unsafe {
            //println!("drop pid {}", self.0);
            PID_ALLOCATOR.lock(place!()).dealloc(self.pid());
        }
    }
}

static PID_ALLOCATOR: SpinLock<PidAllocator> = SpinLock::new(PidAllocator::new(0));

pub fn pid_alloc() -> PidHandle {
    PID_ALLOCATOR.lock(place!()).alloc()
}