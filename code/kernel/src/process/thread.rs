use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicI32, AtomicUsize},
};

use alloc::{string::String, sync::Arc, vec::Vec};
use riscv::register::sstatus;

use crate::{
    memory::{allocator::frame::FrameAllocator, StackID, UserSpace},
    sync::{even_bus::EventBus, mutex::SpinNoIrqLock as Mutex},
    tools::allocator::from_usize_allocator::FromUsize,
    trap::context::UKContext,
};

use super::{pid::pid_alloc, AliveProcess, Process, Tid};

struct ThreadGroup {}

// only run in local thread
pub struct Thread {
    // never change
    pub tid: Tid,
    pub process: Arc<Process>,
    stack_id: StackID,
    // thread local
    inner: UnsafeCell<ThreadInner>,
}

unsafe impl Send for Thread {}
unsafe impl Sync for Thread {}

pub struct ThreadInner {
    uk_context: UKContext,
}

impl Thread {
    pub fn new(elf_data: &[u8], allocator: &mut impl FrameAllocator) -> Arc<Self> {
        let (user_space, stack_id, user_sp, entry_point) =
            UserSpace::from_elf(elf_data, allocator).unwrap();
        let pid = pid_alloc();
        let tid = Tid::from_usize(pid.get_usize());
        let pgid = AtomicUsize::new(pid.get_usize());
        let process = Arc::new(Process {
            pid,
            pgid,
            event_bus: EventBus::new(),
            alive: Mutex::new(None),
            exit_code: AtomicI32::new(0),
        });
        *process.alive.lock(place!()) = Some(AliveProcess {
            user_space,
            cwd: String::new(),
            exec_path: String::new(),
            parent: None,
            children: Vec::new(),
            threads: Vec::new(),
        });
        let mut thread = Self {
            tid,
            process: process.clone(),
            inner: UnsafeCell::new(ThreadInner {
                uk_context: unsafe { UKContext::any() },
            }),
            stack_id,
        };
        let (argc, argv) = (0, 0);
        thread.inner.get_mut().uk_context.exec_init(
            user_sp,
            entry_point,
            sstatus::read(),
            argc,
            argv,
        );
        let ptr = Arc::new(thread);
        process
            .alive_then(|alive| alive.threads.push(Arc::downgrade(&ptr)))
            .unwrap();
        ptr
    }
    pub fn get_context(&self) -> &mut UKContext {
        unsafe { &mut (*self.inner.get()).uk_context }
    }
}
