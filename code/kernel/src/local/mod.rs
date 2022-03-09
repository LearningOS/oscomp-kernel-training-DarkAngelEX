use alloc::{boxed::Box, sync::Arc, vec::Vec};
use riscv::register::sstatus;

use crate::{
    config::PAGE_SIZE,
    hart::{self, cpu, sfence},
    memory::{
        self,
        asid::{Asid, AsidVersion},
        PageTable,
    },
    process::{proc_table, thread::Thread},
    sync::mutex::SpinNoIrqLock as Mutex,
    tools::container::sync_unsafe_cell::SyncUnsafeCell,
    user::UserAccessStatus,
    xdebug::stack_trace::StackTrace,
};

macro_rules! array_repeat {
    ($a: expr) => {
        [
            $a, $a, $a, $a, $a, $a, $a, $a, $a, $a, $a, $a, $a, $a, $a, $a,
        ]
    };
}

static mut HART_LOCAL: [HartLocal; 16] = array_repeat!(HartLocal::new());

/// any hart can only access each unit so didn't need mutex.
///
/// access other local must through function below.
pub struct HartLocal {
    task: Option<Box<TaskLocal>>,
    pub kstack_bottom: usize,
    asid_version: AsidVersion,
    queue: Vec<Box<dyn FnOnce()>>,
    pending: Mutex<Vec<Box<dyn FnOnce()>>>,
}
pub struct TaskLocal {
    pub user_access_status: UserAccessStatus, // 用户访问测试
    // 使用Option可以避免Arc Clone复制的CAS开销，直接移动到OutermostFuture。
    pub thread: Arc<Thread>,
    // 进程改变页表时需要同步到这里，更新回OutermostFuture
    pub page_table: Arc<SyncUnsafeCell<PageTable>>,
    // debug 栈追踪器
    pub stack_trace: StackTrace,
    pub sie_count: usize, // 不为0时关中断
    pub sum_count: usize, // 不为0时允许访问用户数据 必须关中断
}

impl HartLocal {
    const fn new() -> Self {
        Self {
            task: None,
            kstack_bottom: 0,
            asid_version: AsidVersion::first_asid_version(),
            queue: Vec::new(),
            pending: Mutex::new(Vec::new()),
        }
    }
    fn register(&self, f: impl FnOnce() + 'static) {
        self.pending.lock(place!()).push(Box::new(f))
    }
    fn handle(&mut self) {
        debug_check!(self.queue.is_empty());
        // use swap instead of take bucause it can keep reverse space.
        core::mem::swap(&mut self.queue, &mut *self.pending.lock(place!()));
        while let Some(f) = self.queue.pop() {
            f()
        }
    }
    pub fn try_task(&mut self) -> Option<&mut TaskLocal> {
        self.task.as_mut().map(|a| a.as_mut())
    }
    pub fn task(&mut self) -> &mut TaskLocal {
        self.task.as_mut().unwrap().as_mut()
    }
    pub fn set_task(&mut self, task: Box<TaskLocal>) {
        task.set_env();
        assert!(self.task.is_none());
        self.task = Some(task);
    }
    pub fn take_task(&mut self) -> Box<TaskLocal> {
        let ret = self.task.take().unwrap();
        ret.clear_env();
        ret
    }
    pub fn sstatus_zero_assert(&self) {
        let sstatus = sstatus::read();
        assert!(!sstatus.sum());
        assert!(!sstatus.sie());
    }
}

impl TaskLocal {
    pub fn by_initproc() -> Self {
        proc_table::get_initproc()
            .alive_then(|a| Self {
                user_access_status: UserAccessStatus::Forbid,
                thread: a.threads.get_first().unwrap(),
                page_table: a.user_space.page_table_arc(),
                stack_trace: StackTrace::new(),
                sie_count: 0,
                sum_count: 0,
            })
            .unwrap()
    }
    fn set_env(&self) {
        unsafe {
            if self.sie_count > 0 {
                sstatus::clear_sie();
            } else {
                sstatus::set_sie();
            }
            if self.sum_count > 0 {
                assert!(self.sie_count > 0);
                sstatus::set_sum();
            } else {
                sstatus::clear_sum();
            }
            self.page_table.get().using();
        }
    }
    fn clear_env(&self) {
        unsafe {
            sstatus::clear_sie();
            sstatus::clear_sum();
            memory::set_satp_by_global();
        }
    }

    pub fn sum_inc(&mut self) {
        assert!(self.sie_count != 0);
        if self.sum_count == 0 {
            assert!(self.user_access_status.is_forbid());
            self.user_access_status.set_access();
            unsafe { sstatus::set_sum() };
        }
        self.sum_count += 1;
    }
    pub fn sum_dec(&mut self) {
        assert!(self.sie_count != 0);
        assert!(self.sum_count != 0);
        self.sum_count -= 1;
        if self.sum_count == 0 {
            assert!(self.user_access_status.is_access());
            self.user_access_status.set_forbid();
            unsafe { sstatus::clear_sum() };
        }
    }
    pub fn sum_cur(&self) -> usize {
        self.sum_count
    }
    pub fn sie_inc(&mut self) {
        if self.sie_count == 0 {
            unsafe { sstatus::clear_sie() };
        }
        self.sie_count += 1;
    }
    pub fn sie_dec(&mut self) {
        assert!(self.sie_count != 0);
        self.sie_count -= 1;
        if self.sie_count == 0 {
            unsafe { sstatus::set_sie() };
        }
    }
    pub fn sie_cur(&self) -> usize {
        self.sie_count
    }
}

#[inline(always)]
pub fn hart_local() -> &'static mut HartLocal {
    let i = cpu::hart_id();
    unsafe { &mut HART_LOCAL[i] }
}
pub fn task_local() -> &'static mut TaskLocal {
    hart_local().task()
}

fn get_local_by_id(id: usize) -> &'static HartLocal {
    unsafe { &HART_LOCAL[id] }
}

pub fn set_stack() {
    let sp = hart::current_sp();
    hart_local().kstack_bottom = (sp & !(PAGE_SIZE - 1)) + PAGE_SIZE;
}

#[inline(never)]
pub fn stack_size() -> usize {
    let sp = hart::current_sp();
    hart_local().kstack_bottom - sp
}

pub fn handle_current_local() {
    hart_local().handle()
}

pub fn asid_version_update(latest_version: AsidVersion) {
    let cpu_version = &mut hart_local().asid_version;
    if *cpu_version != latest_version {
        assert!(*cpu_version < latest_version);
        sfence::sfence_vma_all_global();
        *cpu_version = latest_version
    }
}

#[inline(always)]
pub fn all_hart_fn(f: impl Fn<(), Output = impl FnOnce() + 'static>) {
    f()();
    let cur = cpu::hart_id();
    for i in 0..cpu::count() {
        if i == cur {
            continue;
        }
        get_local_by_id(i).register(f());
    }
}

pub fn all_hart_fence_i() {
    all_hart_fn(|| || sfence::fence_i());
}

pub fn all_hart_sfence_vma_asid(asid: Asid) {
    all_hart_fn(|| move || sfence::sfence_vma_asid(asid.into_usize()));
}
