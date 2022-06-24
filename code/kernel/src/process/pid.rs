use crate::{
    sync::mutex::SpinLock,
    tools::{
        allocator::{from_usize_allocator::FromUsizeAllocator, Own},
        container::never_clone_linked_list::NeverCloneLinkedList,
        Wrapper,
    },
};

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct Pid(pub usize);

impl Pid {
    pub fn into_usize(self) -> usize {
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

type PidAllocator = FromUsizeAllocator<Pid, PidWrapper, NeverCloneLinkedList<usize>>;

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
            PID_ALLOCATOR.lock().dealloc(self.pid());
        }
    }
}

static PID_ALLOCATOR: SpinLock<PidAllocator> = SpinLock::new(PidAllocator::default());

pub fn pid_alloc() -> PidHandle {
    PID_ALLOCATOR.lock().alloc()
}
