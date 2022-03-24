use core::{
    cell::UnsafeCell,
    future::Future,
    pin::Pin,
    sync::atomic::{AtomicI32, AtomicUsize},
    task::{Context, Poll},
};

use alloc::{
    boxed::Box,
    collections::{BTreeMap, LinkedList},
    string::String,
    sync::{Arc, Weak},
};
use riscv::register::sstatus;

use crate::{
    memory::{address::PageCount, allocator::frame::FrameAllocator, StackID, UserSpace},
    sync::{even_bus::EventBus, mutex::SpinNoIrqLock as Mutex},
    syscall::SysError,
    tools::allocator::from_usize_allocator::{FromUsize, NeverCloneUsizeAllocator},
    trap::context::UKContext,
};

use super::{
    children::ChildrenSet, fd::FdTable, pid::pid_alloc, proc_table, AliveProcess, Process, Tid,
};

pub struct ThreadGroup {
    threads: BTreeMap<Tid, Weak<Thread>>,
    tid_allocator: NeverCloneUsizeAllocator,
}

impl ThreadGroup {
    pub fn new(start: usize) -> Self {
        Self {
            threads: BTreeMap::new(),
            tid_allocator: NeverCloneUsizeAllocator::default().start(start),
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
    pub fn get_first(&self) -> Option<Arc<Thread>> {
        self.threads
            .first_key_value()
            .and_then(|(_tid, thread)| thread.upgrade())
    }
    pub fn alloc_tid(&mut self) -> Tid {
        let x = self.tid_allocator.alloc();
        Tid::from_usize(x)
    }
    pub unsafe fn dealloc_tid(&mut self, tid: Tid) {
        self.tid_allocator.dealloc(tid.to_usize())
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
    uk_context: Box<UKContext>,
}

impl Thread {
    pub fn new_initproc(elf_data: &[u8], allocator: &mut impl FrameAllocator) -> Arc<Self> {
        let (user_space, stack_id, user_sp, entry_point) =
            UserSpace::from_elf(elf_data, PageCount::from_usize(1), allocator).unwrap();
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
                threads: ThreadGroup::new(tid.to_usize() + 1),
                fd_table: FdTable::new(),
                signal_queue: LinkedList::new(),
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
            user_sp.into(),
            entry_point,
            sstatus::read(),
            argc,
            argv,
        );
        let ptr = Arc::new(thread);
        process
            .alive_then(|alive| alive.threads.push(&ptr))
            .unwrap();
        proc_table::insert_proc(&process);
        unsafe { proc_table::set_initproc(process) };
        ptr
    }
    pub fn from_process(process: Arc<Process>, tid: Tid, stack_id: StackID) -> Self {
        Self {
            tid,
            process,
            inner: UnsafeCell::new(ThreadInner {
                stack_id,
                uk_context: unsafe { UKContext::any() },
            }),
        }
    }
    #[allow(clippy::mut_from_ref)]
    pub fn inner(&self) -> &mut ThreadInner {
        unsafe { &mut *self.inner.get() }
    }
    #[allow(clippy::mut_from_ref)]
    pub fn get_context(&self) -> &mut UKContext {
        unsafe { &mut (*self.inner.get()).uk_context }
    }

    pub fn fork(&self) -> Result<Arc<Self>, SysError> {
        let new_process = self.process.fork(self.tid)?;
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

pub fn yield_now() -> impl Future<Output = ()> {
    YieldFuture { flag: false }
}

struct YieldFuture {
    flag: bool,
}

impl Future for YieldFuture {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        if self.flag {
            Poll::Ready(())
        } else {
            self.flag = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}
