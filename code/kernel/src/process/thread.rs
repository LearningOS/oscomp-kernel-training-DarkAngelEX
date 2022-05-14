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
    vec::Vec,
};
use riscv::register::sstatus;

use crate::{
    fs::VfsInode,
    hart::floating,
    memory::{self, address::PageCount, user_ptr::UserInOutPtr, StackID, UserSpace},
    signal::SignalSet,
    sync::{even_bus::EventBus, mutex::SpinNoIrqLock as Mutex},
    syscall::SysError,
    tools::allocator::from_usize_allocator::{FromUsize, NeverCloneUsizeAllocator},
    trap::context::UKContext,
};

use super::{
    children::ChildrenSet, fd::FdTable, pid::pid_alloc, proc_table, AliveProcess, CloneFlag,
    Process, Tid,
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
    pub set_child_tid: UserInOutPtr<u32>,
    pub clear_child_tid: UserInOutPtr<u32>,
    pub signal_mask: SignalSet,
    uk_context: Box<UKContext>,
}

impl Thread {
    pub fn new_initproc(
        cwd: Arc<VfsInode>,
        elf_data: &[u8],
        args: Vec<String>,
        envp: Vec<String>,
    ) -> Arc<Self> {
        let reverse_stack = PageCount::from_usize(2);
        let (user_space, stack_id, user_sp, entry_point, auxv) =
            UserSpace::from_elf(elf_data, reverse_stack).unwrap();
        unsafe { user_space.raw_using() };
        let (user_sp, argc, argv, xenvp) =
            user_space.push_args(user_sp.into(), &args, &envp, &auxv, reverse_stack);
        memory::set_satp_by_global();
        drop(args);
        let pid = pid_alloc();
        let tid = Tid::from_usize(pid.get_usize());
        let pgid = AtomicUsize::new(pid.get_usize());
        let process = Arc::new(Process {
            pid,
            pgid,
            event_bus: EventBus::new(),
            alive: Mutex::new(Some(AliveProcess {
                user_space,
                cwd,
                exec_path: String::new(),
                envp,
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
                set_child_tid: UserInOutPtr::null(),
                clear_child_tid: UserInOutPtr::null(),
                signal_mask: SignalSet::empty(),
                uk_context: unsafe { UKContext::any() },
            }),
        };
        thread.inner.get_mut().uk_context.exec_init(
            user_sp,
            entry_point,
            sstatus::read(),
            floating::default_fcsr(),
            argc,
            argv,
            xenvp,
        );
        let ptr = Arc::new(thread);
        process
            .alive_then(|alive| alive.threads.push(&ptr))
            .unwrap();
        proc_table::insert_proc(&process);
        unsafe { proc_table::set_initproc(process) };
        ptr
    }
    #[allow(clippy::mut_from_ref)]
    pub fn inner(&self) -> &mut ThreadInner {
        unsafe { &mut *self.inner.get() }
    }
    #[allow(clippy::mut_from_ref)]
    pub fn get_context(&self) -> &mut UKContext {
        unsafe { &mut (*self.inner.get()).uk_context }
    }

    pub fn clone_impl(
        &self,
        flag: CloneFlag,
        new_sp: usize,
        _parent_tidptr: UserInOutPtr<u32>,
        child_tidptr: UserInOutPtr<u32>,
    ) -> Result<Arc<Self>, SysError> {
        let new_process = self.process.fork(self.tid)?;
        let inner = self.inner();

        let set_child_tid = if flag.contains(CloneFlag::CLONE_CHILD_SETTID) {
            child_tidptr
        } else {
            UserInOutPtr::null()
        };
        let clear_child_tid = if flag.contains(CloneFlag::CLONE_CHILD_CLEARTID) {
            child_tidptr
        } else {
            UserInOutPtr::null()
        };
        let signal_mask = inner.signal_mask;
        let thread = Arc::new(Self {
            tid: self.tid,
            process: new_process,
            inner: UnsafeCell::new(ThreadInner {
                stack_id: inner.stack_id,
                set_child_tid,
                clear_child_tid,
                signal_mask,
                uk_context: inner.uk_context.fork(),
            }),
        });
        if new_sp != 0 {
            thread.inner().uk_context.set_user_sp(new_sp);
        }
        thread.inner().uk_context.set_user_a0(0);
        thread
            .process
            .alive_then(|a| a.threads.push(&thread))
            .unwrap();
        Ok(thread)
    }
}

pub async fn yield_now() {
    YieldFuture(false).await
}

struct YieldFuture(bool);

impl Future for YieldFuture {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        use core::sync::atomic;
        if self.0 {
            Poll::Ready(())
        } else {
            self.0 = true;
            atomic::fence(atomic::Ordering::SeqCst);
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}
