use core::{
    cell::UnsafeCell,
    future::Future,
    pin::Pin,
    sync::atomic::{AtomicI32, AtomicUsize},
    task::{Context, Poll},
};

use alloc::{
    collections::BTreeMap,
    string::String,
    sync::{Arc, Weak},
    vec::Vec,
};
use ftl_util::error::SysR;
use riscv::register::sstatus::{self, SPP};
use vfs::VfsFile;

use crate::{
    futex::{Futex, FutexIndex, RobustListHead, WakeStatus, FUTEX_BITSET_MATCH_ANY},
    hart::floating,
    local,
    memory::{
        self,
        address::{PageCount, UserAddr},
        user_ptr::UserInOutPtr,
        UserSpace,
    },
    signal::{
        context::SignalContext,
        manager::{ProcSignalManager, ThreadSignalManager},
        Sig,
    },
    sync::{even_bus::EventBus, mutex::SpinNoIrqLock as Mutex},
    timer,
    trap::context::UKContext,
    user::check::UserCheck,
    xdebug::PRINT_SYSCALL_ALL,
};

use super::{
    children::ChildrenSet,
    fd::FdTable,
    resource::{ProcessTimer, ThreadTimer},
    search,
    tid::TidHandle,
    AliveProcess, CloneFlag, Dead, Process, Tid,
};

pub struct ThreadGroup {
    threads: BTreeMap<Tid, Weak<Thread>>,
}

impl ThreadGroup {
    pub fn new() -> Self {
        Self {
            threads: BTreeMap::new(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.threads.is_empty()
    }
    pub fn iter(&self) -> impl Iterator<Item = Arc<Thread>> + '_ {
        self.threads.iter().map(|(_id, td)| td.upgrade().unwrap())
    }
    pub fn push(&mut self, thread: &Arc<Thread>) {
        match self
            .threads
            .try_insert(thread.tid.tid(), Arc::downgrade(thread))
        {
            Ok(_) => (),
            Err(_) => panic!("double insert tid"),
        }
    }
    pub fn remove(&mut self, tid: Tid) {
        let _ = self.threads.remove(&tid);
    }
    pub fn map(&self, tid: Tid) -> Option<Arc<Thread>> {
        self.threads
            .get_key_value(&tid)
            .map(|(_, th)| th.upgrade().unwrap())
    }
    pub fn len(&self) -> usize {
        self.threads.len()
    }
    pub fn get_first(&self) -> Option<Arc<Thread>> {
        self.threads
            .first_key_value()
            .and_then(|(_tid, thread)| thread.upgrade())
    }
}

// only run in local thread
pub struct Thread {
    // never change
    tid: TidHandle,
    pub process: Arc<Process>,
    // thread local
    inner: UnsafeCell<ThreadInner>,
}

impl Thread {
    pub fn receive(&self, sig: Sig) {
        self.inner().signal_manager.receive(sig);
    }
    /// 此函数将在线程首次进入用户态前执行一次, 忽略页错误
    pub async fn settid(&self) {
        if let Some(ptr) = self.inner().set_child_tid.nonnull_mut() {
            if PRINT_SYSCALL_ALL {
                println!("settid: {:#x}", ptr.as_usize());
            }
            if let Ok(buf) = UserCheck::new(&self.process).writable_value(ptr).await {
                buf.store(self.tid().0 as u32)
            }
        }
    }
    /// 此函数将在线程首次进入用户态前执行一次, 忽略页错误
    pub async fn cleartid(&self) {
        stack_trace!();
        if let Some(ptr) = self.inner().clear_child_tid.nonnull_mut() {
            while let Ok(buf) = UserCheck::new(&self.process).writable_value(ptr).await {
                if PRINT_SYSCALL_ALL {
                    println!("cleartid: {:#x}", ptr.as_usize());
                }
                buf.store(0);
                let futex = self.fetch_futex(ptr.as_uptr().unwrap());
                match futex.wake(FUTEX_BITSET_MATCH_ANY, 1, None, || false) {
                    WakeStatus::Ok(_) => (),
                    WakeStatus::Closed => continue,
                    WakeStatus::Fail => panic!(),
                }
                break;
            }
        }
    }
    pub fn exit_send_signal(&self) -> Option<Sig> {
        self.inner().exit_signal
    }
    pub fn timer(&self) -> &ThreadTimer {
        &self.inner().timer
    }
    pub fn timer_enter_thread(&self) {
        let timer = &mut self.inner().timer;
        timer.enter_thread(timer::now());
    }
    pub fn timer_leave_thread(&self) {
        let timer = &mut self.inner().timer;
        timer.leave_thread(timer::now());
        timer.maybe_submit(&self.process);
    }
    /// 刷新当前线程的计时器并提交至进程
    pub fn timer_fence(&self) {
        let timer = &mut self.inner().timer;
        timer.timer_fence(timer::now());
        timer.submit(&mut *self.process.timer.lock());
    }
    pub fn timer_into_user(&self) {
        let timer = &mut self.inner().timer;
        timer.enter_user(timer::now());
        timer.maybe_submit(&self.process);
    }
    pub fn timer_leave_user(&self) {
        let timer = &mut self.inner().timer;
        timer.leave_user(timer::now());
        timer.maybe_submit(&self.process);
    }
}

impl Drop for Thread {
    fn drop(&mut self) {
        let tid = self.tid();
        let _ = self.process.alive_then(move |a| a.threads.remove(tid));
        search::clear_thread(self.tid());
    }
}

unsafe impl Send for Thread {}
unsafe impl Sync for Thread {}

pub struct ThreadInner {
    pub signal_manager: ThreadSignalManager,
    /// 信号返回上下文指针
    pub scx_ptr: UserInOutPtr<SignalContext>,
    /// 当前用户上下文
    pub uk_context: UKContext,
    /// 根据clone标志决定是否将此地址写入tid
    pub set_child_tid: UserInOutPtr<u32>,
    /// 如果线程退出时值非0则向此地址写入0并执行
    /// futex(clear_child_tid, FUTEX_WAKE, 1, NULL, NULL, 0)
    pub clear_child_tid: UserInOutPtr<u32>,
    /// 线程局部指针
    pub tls: UserInOutPtr<u8>,
    /// 进程退出后将向父进程发送此信号
    pub exit_signal: Option<Sig>,
    pub robust_list: UserInOutPtr<RobustListHead>,
    pub futex_index: FutexIndex,
    /// 通过exit退出的线程将为true
    pub exited: bool,
    /// 时间统计
    pub timer: ThreadTimer,
}

impl Thread {
    pub fn new_initproc(
        cwd: Arc<VfsFile>,
        elf_data: &[u8],
        args: Vec<String>,
        envp: Vec<String>,
    ) -> Arc<Self> {
        let reverse_stack = PageCount(2);
        let (user_space, user_sp, entry_point, auxv) =
            UserSpace::from_elf(elf_data, reverse_stack).unwrap();
        unsafe { user_space.raw_using() };
        let (user_sp, argc, argv, xenvp) =
            user_space.push_args(user_sp, &args, &envp, &auxv, reverse_stack);
        memory::set_satp_by_global();
        let (tid, pid) = super::tid::alloc_tid_pid();
        let pgid = AtomicUsize::new(pid.get_usize());
        let process = Arc::new(Process {
            pid,
            pgid,
            event_bus: EventBus::new(),
            signal_manager: ProcSignalManager::new(),
            alive: Mutex::new(Some(AliveProcess {
                user_space,
                cwd,
                exec_path: String::new(),
                parent: None,
                children: ChildrenSet::new(),
                threads: ThreadGroup::new(),
                fd_table: FdTable::new(),
            })),
            exit_code: AtomicI32::new(i32::MIN),
            timer: Mutex::new(ProcessTimer::ZERO),
        });
        let mut thread = Self {
            tid,
            process: process.clone(),
            inner: UnsafeCell::new(ThreadInner {
                signal_manager: ThreadSignalManager::new(),
                scx_ptr: UserInOutPtr::null(),
                uk_context: UKContext::new(),

                set_child_tid: UserInOutPtr::null(),
                clear_child_tid: UserInOutPtr::null(),
                tls: UserInOutPtr::null(),
                exit_signal: None,
                robust_list: UserInOutPtr::null(),
                futex_index: FutexIndex::new(),
                exited: false,
                timer: ThreadTimer::ZERO,
            }),
        };
        let mut sstatus = sstatus::read();
        sstatus.set_sie(false);
        sstatus.set_spie(false);
        sstatus.set_spp(SPP::User);
        thread.inner.get_mut().uk_context.exec_init(
            user_sp,
            entry_point,
            sstatus,
            floating::default_fcsr(),
            (argc, argv, xenvp),
        );
        let thread = Arc::new(thread);
        process
            .alive_then(|alive| alive.threads.push(&thread))
            .unwrap();
        search::insert_proc(&process);
        search::insert_thread(&thread);
        unsafe { search::set_initproc(process) };
        thread
    }
    #[inline(always)]
    pub fn tid(&self) -> Tid {
        self.tid.tid()
    }
    #[allow(clippy::mut_from_ref)]
    pub fn inner(&self) -> &mut ThreadInner {
        unsafe { &mut *self.inner.get() }
    }
    #[allow(clippy::mut_from_ref)]
    pub fn get_context(&self) -> &mut UKContext {
        unsafe { &mut (*self.inner.get()).uk_context }
    }
    #[inline]
    pub async fn handle_signal(&self) -> Result<(), Dead> {
        crate::signal::handle_signal(self.inner(), &self.process).await
    }
    pub fn fork_impl(
        &self,
        flag: CloneFlag,
        new_sp: usize,
        set_child_tid: UserInOutPtr<u32>,
        clear_child_tid: UserInOutPtr<u32>,
        tls: Option<UserInOutPtr<u8>>,
        exit_signal: u32,
    ) -> SysR<Arc<Self>> {
        debug_assert!(!flag.contains(CloneFlag::CLONE_THREAD));
        let (tid, pid) = super::tid::alloc_tid_pid();
        let process = self.process.fork(pid)?;
        let inner = self.inner();
        let thread = Arc::new(Self {
            tid,
            process,
            inner: UnsafeCell::new(ThreadInner {
                signal_manager: inner.signal_manager.fork(),
                scx_ptr: UserInOutPtr::null(),
                uk_context: inner.uk_context.fork(tls.map(|v| v.as_usize())),
                set_child_tid,
                clear_child_tid,
                tls: tls.unwrap_or(inner.tls),
                exit_signal: Sig::from_user(exit_signal).ok(),
                robust_list: inner.robust_list,
                futex_index: inner.futex_index.fork(),
                exited: false,
                timer: ThreadTimer::ZERO,
            }),
        });
        search::insert_thread(&thread);
        if new_sp != 0 {
            thread.inner().uk_context.set_user_sp(new_sp);
        }
        thread.inner().uk_context.set_user_a0(0);
        thread
            .process
            .alive_then(|a| a.threads.push(&thread))
            .unwrap();
        local::all_hart_fence_i();
        Ok(thread)
    }
    pub fn clone_thread(
        &self,
        flag: CloneFlag,
        new_sp: usize,
        set_child_tid: UserInOutPtr<u32>,
        clear_child_tid: UserInOutPtr<u32>,
        tls: Option<UserInOutPtr<u8>>,
        exit_signal: u32,
    ) -> SysR<Arc<Self>> {
        stack_trace!();
        debug_assert!(flag.contains(CloneFlag::CLONE_THREAD));
        debug_assert!(new_sp != 0);
        let tid = super::tid::alloc_tid_own();
        let process = self.process.clone();
        let inner = self.inner();
        let thread = Arc::new(Self {
            tid,
            process,
            inner: UnsafeCell::new(ThreadInner {
                signal_manager: inner.signal_manager.fork(),
                scx_ptr: UserInOutPtr::null(),
                uk_context: inner.uk_context.fork(tls.map(|v| v.as_usize())),
                set_child_tid,
                clear_child_tid,
                tls: tls.unwrap_or(inner.tls),
                exit_signal: Sig::from_user(exit_signal).ok(),
                robust_list: inner.robust_list,
                futex_index: inner.futex_index.fork(),
                exited: false,
                timer: ThreadTimer::ZERO,
            }),
        });
        search::insert_thread(&thread);
        if new_sp != 0 {
            thread.inner().uk_context.set_user_sp(new_sp);
        }
        thread.inner().uk_context.set_user_a0(0);
        thread
            .process
            .alive_then(|a| a.threads.push(&thread))
            .unwrap();
        // 不需要刷新指令缓存
        Ok(thread)
    }

    pub fn fetch_futex(&self, ua: UserAddr<u32>) -> Arc<Futex> {
        stack_trace!();
        if let Some(fx) = self.inner().futex_index.try_fetch(ua) && !fx.closed() {
            return fx;
        }
        let fx = self
            .process
            .alive_then(|a| a.user_space.fetch_futex(ua).take_arc())
            .unwrap();
        self.inner().futex_index.insert(ua, Arc::downgrade(&fx));
        fx
    }
    pub fn try_fetch_futex(&self, ua: UserAddr<u32>) -> Option<Arc<Futex>> {
        stack_trace!();
        if let Some(fx) = self.inner().futex_index.try_fetch(ua) {
            return Some(fx);
        }
        let fx = self
            .process
            .alive_then(|a| a.user_space.try_fetch_futex(ua).map(|p| p.take_arc()))
            .unwrap();
        if let Some(fx) = fx.as_ref() {
            self.inner().futex_index.insert(ua, Arc::downgrade(fx));
        }
        fx
    }
}

pub async fn yield_now() {
    YieldFuture(false).await
}

struct YieldFuture(bool);

impl Future for YieldFuture {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        if self.0 {
            return Poll::Ready(());
        }
        self.0 = true;
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}
