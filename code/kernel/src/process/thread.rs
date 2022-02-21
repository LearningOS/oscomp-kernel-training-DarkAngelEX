use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicI32, AtomicUsize},
};

use alloc::{
    collections::BTreeMap,
    string::String,
    sync::{Arc, Weak},
};
use riscv::register::sstatus;

use crate::{
    memory::{allocator::frame::FrameAllocator, StackID, UserSpace},
    sync::{even_bus::EventBus, mutex::SpinNoIrqLock as Mutex},
    tools::{allocator::from_usize_allocator::FromUsize, error::FrameOutOfMemory},
    trap::context::UKContext,
};

use super::{children::ChildrenSet, pid::pid_alloc, AliveProcess, Process, Tid};

pub struct ThreadGroup {
    threads: BTreeMap<Tid, Weak<Thread>>,
}

impl ThreadGroup {
    pub fn new() -> Self {
        Self {
            threads: BTreeMap::new(),
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = Arc<Thread>> + '_ {
        self.threads.iter().map(|(_id, td)| td.upgrade().unwrap())
    }
    pub fn push(&mut self, thread: &Arc<Thread>) {
        match self.threads.try_insert(thread.tid, Arc::downgrade(thread)) {
            Ok(_) => (),
            Err(_) => panic!("double insert tid"),
        }
    }
    pub fn map(&self, tid: Tid) -> Option<Arc<Thread>> {
        self.threads
            .get_key_value(&tid)
            .map(|(_, th)| th.upgrade().unwrap())
    }
    pub unsafe fn clear_thread_except(&mut self, tid: Tid) {
        self.threads.retain(|&xtid, _b| xtid == tid);
        assert!(self.threads.len() == 1);
    }
    pub fn len(&self) -> usize {
        self.threads.len()
    }
}

// only run in local thread
pub struct Thread {
    // never change
    pub tid: Tid,
    pub process: Arc<Process>,
    // thread local
    inner: UnsafeCell<ThreadInner>,
}

unsafe impl Send for Thread {}
unsafe impl Sync for Thread {}

pub struct ThreadInner {
    pub stack_id: StackID,
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
            alive: Mutex::new(Some(AliveProcess {
                user_space,
                cwd: String::new(),
                exec_path: String::new(),
                parent: None,
                children: ChildrenSet::new(),
                threads: ThreadGroup::new(),
            })),
            exit_code: AtomicI32::new(0),
        });
        let mut thread = Self {
            tid,
            process: process.clone(),
            inner: UnsafeCell::new(ThreadInner {
                stack_id,
                uk_context: unsafe { UKContext::any() },
            }),
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
            .alive_then(|alive| alive.threads.push(&ptr))
            .unwrap();
        ptr
    }
    pub fn inner(&self) -> &mut ThreadInner {
        unsafe { &mut *self.inner.get() }
    }
    pub fn get_context(&self) -> &mut UKContext {
        unsafe { &mut (*self.inner.get()).uk_context }
    }

    pub fn fork(&self, allocator: &mut impl FrameAllocator) -> Result<Arc<Self>, FrameOutOfMemory> {
        let new_process = self.process.fork(self.tid, allocator)?;
        let thread = Arc::new(Self {
            tid: self.tid,
            process: new_process,
            inner: UnsafeCell::new(ThreadInner {
                stack_id: self.inner().stack_id,
                uk_context: self.inner().uk_context.fork(),
            }),
        });
        thread.inner().uk_context.set_user_a0(0);
        thread
            .process
            .alive_then(|a| a.threads.push(&thread))
            .unwrap();
        Ok(thread)
    }
}