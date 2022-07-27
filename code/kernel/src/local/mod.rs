use core::sync::atomic::Ordering;

use alloc::{boxed::Box, vec::Vec};
use ftl_util::local::FTLCPULocal;
use riscv::register::sstatus;

use crate::{
    config::PAGE_SIZE,
    hart::{self, cpu, floating, sbi, sfence},
    memory::{
        self,
        address::UserAddr4K,
        allocator::LocalHeap,
        asid::{Asid, AsidVersion},
        rcu::LocalRcuManager,
    },
    sync::mutex::SpinNoIrqLock,
};

use self::{always_local::AlwaysLocal, task_local::TaskLocal};

pub mod always_local;
pub mod task_local;

#[allow(clippy::declare_interior_mutable_const)]
const HART_LOCAL_EACH: HartLocal = HartLocal::new();
static mut HART_LOCAL: [HartLocal; 16] = [HART_LOCAL_EACH; 16];

#[repr(align(64))]
struct Align64;

/// any hart can only access each unit so didn't need mutex.
///
/// access other local must through function below.
///
/// use align(64) to avoid false share
#[repr(C)]
#[repr(align(64))]
pub struct HartLocal {
    ftl_cpulocal: FTLCPULocal,
    enable: bool,
    always_local: AlwaysLocal,
    local_now: LocalNow,
    kstack_bottom: usize,
    pub interrupt: bool,
    asid_version: AsidVersion,
    queue: Vec<Box<dyn FnOnce()>>,
    pub in_exception: bool, // forbid exception nest
    pub local_heap: LocalHeap,
    pub local_rcu: LocalRcuManager,
    _align64: Align64, // 让mailbox不会和其他部分共享cacheline
    mailbox: SpinNoIrqLock<Vec<Box<dyn FnOnce()>>>,
    pub idle: bool,
}

unsafe impl Send for HartLocal {}
unsafe impl Sync for HartLocal {}

pub enum LocalNow {
    Idle,
    Task(Box<TaskLocal>),
}

unsafe impl Send for LocalNow {}
unsafe impl Sync for LocalNow {}

impl LocalNow {
    #[inline(always)]
    pub fn always(&mut self, idle: *mut AlwaysLocal) -> &mut AlwaysLocal {
        match self {
            LocalNow::Idle => unsafe { &mut *idle },
            LocalNow::Task(t) => &mut t.always_local,
        }
    }
    #[inline(always)]
    pub fn always_ref(&self, idle: *const AlwaysLocal) -> &AlwaysLocal {
        match self {
            LocalNow::Idle => unsafe { &*idle },
            LocalNow::Task(t) => &t.always_local,
        }
    }
    #[inline(always)]
    pub fn task(&mut self) -> &mut TaskLocal {
        match self {
            LocalNow::Idle => panic!(),
            LocalNow::Task(task) => task.as_mut(),
        }
    }
}

impl HartLocal {
    const fn new() -> Self {
        Self {
            ftl_cpulocal: FTLCPULocal::new(usize::MAX),
            enable: false,
            always_local: AlwaysLocal::new(),
            local_now: LocalNow::Idle,
            queue: Vec::new(),
            mailbox: SpinNoIrqLock::new(Vec::new()),
            kstack_bottom: 0,
            asid_version: AsidVersion::first_asid_version(),
            interrupt: false,
            in_exception: false,
            _align64: Align64,
            local_heap: LocalHeap::new(),
            local_rcu: LocalRcuManager::new(),
            idle: false,
        }
    }
    pub unsafe fn set_hartid(&self, cpuid: usize) {
        self.ftl_cpulocal.cpuid.store(cpuid, Ordering::Release);
    }
    pub fn cpuid(&self) -> usize {
        unsafe { *self.ftl_cpulocal.cpuid.as_mut_ptr() }
    }
    fn register(&self, f: impl FnOnce() + 'static) {
        if self.enable {
            self.mailbox.lock().push(Box::new(f))
        }
    }
    pub fn handle(&mut self) {
        debug_assert!(self.queue.is_empty());
        // use swap instead of take bucause it can keep reverse space.
        if unsafe { self.mailbox.unsafe_get().is_empty() } {
            return;
        }
        core::mem::swap(&mut self.queue, &mut *self.mailbox.lock());
        while let Some(f) = self.queue.pop() {
            f()
        }
    }
    #[inline(always)]
    pub fn task(&mut self) -> &mut TaskLocal {
        self.local_now.task()
    }
    #[inline(always)]
    pub fn always(&mut self) -> &mut AlwaysLocal {
        self.local_now.always(&mut self.always_local)
    }
    #[inline(always)]
    pub fn always_ref(&self) -> &AlwaysLocal {
        self.local_now.always_ref(&self.always_local)
    }
    pub fn enter_task_switch(&mut self, task: &mut LocalNow) {
        assert!(matches!(&mut self.local_now, LocalNow::Idle));
        assert!(matches!(task, LocalNow::Task(_)));
        let new = task.always(&mut self.always_local); // 获取task
        let old = self.always();
        // 关中断 避免中断时使用always_local时发生错误
        if old.sie_cur() == 0 {
            unsafe { sstatus::clear_sie() };
        }
        let open_intrrupt = AlwaysLocal::env_change(new, old);
        unsafe { task.task().page_table.get().using() }
        core::mem::swap(&mut self.local_now, task);
        if open_intrrupt {
            unsafe { sstatus::set_sie() };
        }
    }
    pub fn leave_task_switch(&mut self, task: &mut LocalNow) {
        assert!(matches!(&mut self.local_now, LocalNow::Task(_)));
        assert!(matches!(task, LocalNow::Idle));
        // save floating register
        floating::switch_out(&mut self.task().thread.get_context().user_fx);
        let new = task.always(&mut self.always_local);
        let old = self.always();
        if old.sie_cur() == 0 {
            unsafe { sstatus::clear_sie() };
        }
        let open_intrrupt = AlwaysLocal::env_change(new, old);
        memory::set_satp_by_global();
        core::mem::swap(&mut self.local_now, task);
        if open_intrrupt {
            unsafe { sstatus::set_sie() };
        }
    }
    pub fn switch_kernel_task(&mut self, task: &mut AlwaysLocal) {
        debug_assert!(matches!(&mut self.local_now, LocalNow::Idle));
        let old = self.always();
        // 关中断 避免中断时使用always_local时发生错误
        if old.sie_cur() == 0 {
            unsafe { sstatus::clear_sie() };
        }
        let open_intrrupt = AlwaysLocal::env_change(task, old);
        core::mem::swap(&mut self.always_local, task);
        if open_intrrupt {
            unsafe { sstatus::set_sie() };
        }
    }
    pub fn enter_idle(&mut self) {
        self.idle = true;
    }
    pub fn leave_idle(&mut self) {
        self.idle = false;
    }
}
pub fn init() {
    let local = hart_local();
    local.enable = true;
    local.local_rcu.init_id(local.cpuid());
}
pub unsafe fn bind_tp(hartid: usize) -> usize {
    let hart_local = get_local_by_id(hartid);
    hart_local.set_hartid(hartid);
    hart_local as *const _ as usize
}
#[inline(always)]
pub fn hart_local() -> &'static mut HartLocal {
    unsafe { &mut *(cpu::get_tp() as *mut HartLocal) }
}
#[inline(always)]
pub fn always_local() -> &'static mut AlwaysLocal {
    hart_local().always()
}
#[inline(always)]
pub fn task_local() -> &'static mut TaskLocal {
    hart_local().task()
}

#[inline(always)]
pub unsafe fn get_local_by_id(id: usize) -> &'static HartLocal {
    &HART_LOCAL[id]
}

pub unsafe fn cpu_local_in_use() -> &'static [HartLocal] {
    &HART_LOCAL[cpu::hart_range()]
}

pub fn set_stack() {
    let sp = hart::current_sp();
    // round up 4KB
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
        *cpu_version = latest_version;
        sfence::sfence_vma_all_global();
    }
}

#[inline(always)]
pub fn all_hart_fn(f: impl Fn<(), Output = impl FnOnce() + 'static>) {
    let cur = cpu::hart_id();
    for i in cpu::hart_range() {
        if i == cur {
            continue;
        }
        unsafe { get_local_by_id(i).register(f()) };
    }
    f()();
}

pub fn all_hart_fence_i() {
    all_hart_fn(|| sfence::fence_i);
}

pub fn all_hart_sfence_vma_asid(asid: Asid) {
    all_hart_fn(|| move || sfence::sfence_vma_asid(asid.into_usize()));
}

pub fn all_hart_sfence_vma_va_asid(va: UserAddr4K, asid: Asid) {
    all_hart_fn(|| move || sfence::sfence_vma_va_asid(va.into_usize(), asid.into_usize()));
}

pub fn all_hart_sfence_vma_all_no_global() {
    all_hart_fn(|| sfence::sfence_vma_all_no_global);
}

pub fn all_hart_sfence_vma_va_global(va: UserAddr4K) {
    all_hart_fn(|| move || sfence::sfence_vma_va_global(va.into_usize()));
}

pub fn try_wake_idle_hart() {
    let this_cpu = hart_local().cpuid();
    unsafe {
        for cur in cpu_local_in_use().iter() {
            let cur_id = cur.cpuid();
            if cur_id == this_cpu {
                continue;
            }
            if cur.idle {
                // println!("send ipi: {} -> {}", this_cpu, cur_id);
                let r = sbi::send_ipi(1 << cur_id);
                assert_eq!(r, 0);
                break;
            }
        }
    }
}
