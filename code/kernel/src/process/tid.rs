use crate::{
    sync::mutex::SpinNoIrqLock,
    tools::{
        allocator::{from_usize_allocator::FromUsizeAllocator, Own},
        container::never_clone_linked_list::NeverCloneLinkedList,
        Wrapper,
    },
};

use super::{pid::PidHandle, Pid};

/// fork产生的线程的Tid将被PidHandle释放
///
/// pthread_create产生的线程的Tid被TidHandle释放
///
#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct Tid(pub usize);

from_usize_impl!(Tid);

struct TidWrapper;
impl Wrapper<Tid> for TidWrapper {
    type Output = TidHandle;
    fn wrapper(a: Tid) -> TidHandle {
        TidHandle(a, true)
    }
}

type TidAllocator = FromUsizeAllocator<Tid, TidWrapper, NeverCloneLinkedList<usize>>;

static TID_ALLOCATOR: SpinNoIrqLock<TidAllocator> = SpinNoIrqLock::new(TidAllocator::default());

/// 给新进程分配 PID
pub fn alloc_tid_pid() -> (TidHandle, PidHandle) {
    let mut th = TID_ALLOCATOR.lock().alloc();
    th.1 = false;
    let ph = unsafe { super::pid::pid_alloc_by_tid(th.0) };
    (th, ph)
}

/// 给新线程分配 TID
pub fn alloc_tid_own() -> TidHandle {
    TID_ALLOCATOR.lock().alloc()
}

pub(super) unsafe fn pidhandle_dealloc_impl(pid: Pid) {
    TID_ALLOCATOR.lock().dealloc(Tid(pid.0));
}

#[derive(Debug)]
pub struct TidHandle(Tid, bool);

impl Own<Tid> for TidHandle {}

impl TidHandle {
    #[inline(always)]
    pub fn tid(&self) -> Tid {
        self.0
    }
}

impl Drop for TidHandle {
    fn drop(&mut self) {
        unsafe {
            //println!("drop tid {}", self.0);
            if self.1 {
                TID_ALLOCATOR.lock().dealloc(self.tid());
            }
        }
    }
}
