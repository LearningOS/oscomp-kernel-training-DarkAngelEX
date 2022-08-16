use core::sync::atomic::{AtomicBool, Ordering};

use alloc::boxed::Box;
use ftl_util::local::FTLCPULocal;
use riscv::register::sstatus;

use crate::{
    config::PAGE_SIZE,
    executor,
    hart::{self, cpu, floating, sbi, sfence},
    memory::{
        self,
        address::UserAddr4K,
        allocator::LocalHeap,
        asid::{Asid, AsidVersion, USING_ASID},
        rcu::LocalRcuManager,
    },
    sync::mutex::SpinNoIrqLock,
};

use self::{
    always_local::AlwaysLocal,
    mailbox::{HartMailBox, MailEvent},
    task_local::TaskLocal,
};

pub mod always_local;
mod mailbox;
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
    pub in_exception: bool, // forbid exception nest
    pub local_heap: LocalHeap,
    pub local_rcu: LocalRcuManager,
    local_mail: HartMailBox,
    _align64: Align64, // 让mailbox不会和其他部分共享cacheline
    mailbox: SpinNoIrqLock<HartMailBox>,
    pub sleep: AtomicBool,
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
    pub fn is_idle(&self) -> bool {
        matches!(self, LocalNow::Idle)
    }
    #[inline(always)]
    fn always(&mut self, idle: *mut AlwaysLocal) -> &mut AlwaysLocal {
        match self {
            LocalNow::Idle => unsafe { &mut *idle },
            LocalNow::Task(t) => &mut t.always_local,
        }
    }
    #[inline(always)]
    fn always_ref(&self, idle: *const AlwaysLocal) -> &AlwaysLocal {
        match self {
            LocalNow::Idle => unsafe { &*idle },
            LocalNow::Task(t) => &t.always_local,
        }
    }
    #[inline(always)]
    fn task(&mut self) -> &mut TaskLocal {
        match self {
            LocalNow::Task(task) => task.as_mut(),
            LocalNow::Idle => panic!(),
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
            local_mail: HartMailBox::new(),
            mailbox: SpinNoIrqLock::new(HartMailBox::new()),
            kstack_bottom: 0,
            asid_version: AsidVersion::first_asid_version(),
            interrupt: false,
            in_exception: false,
            _align64: Align64,
            local_heap: LocalHeap::new(),
            local_rcu: LocalRcuManager::new(),
            sleep: AtomicBool::new(false),
        }
    }
    pub unsafe fn set_hartid(&self, cpuid: usize) {
        self.ftl_cpulocal.cpuid.store(cpuid, Ordering::Release);
    }
    pub fn cpuid(&self) -> usize {
        unsafe { *self.ftl_cpulocal.cpuid.as_mut_ptr() }
    }
    fn register(&self, f: impl FnOnce(&mut HartMailBox)) {
        if self.enable {
            f(&mut *self.mailbox.lock())
        }
    }
    /// 处理其他CPU发送到这个CPU的信息, 例如fence.i, sfence.vma等
    #[inline]
    pub fn handle(&mut self) {
        debug_assert!(self.local_mail.is_empty());
        // use swap instead of take bucause it can keep reverse space.
        if unsafe { self.mailbox.unsafe_get().is_empty() } {
            return;
        }
        self.mailbox.lock().swap(&mut self.local_mail);
        self.local_mail.handle();
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
        debug_assert!(self.local_now.is_idle());
        debug_assert!(!task.is_idle());
        let new = task.always(&mut self.always_local); // 获取task
        let old = self.always();
        // 关中断 避免中断时使用always_local时发生错误
        if old.sie_cur() == 0 {
            unsafe { sstatus::clear_sie() };
        }
        let open_intrrupt = AlwaysLocal::env_change(new, old);
        unsafe { task.task().page_table.get().using() }
        task.task().thread.timer_enter_thread();
        core::mem::swap(&mut self.local_now, task);
        if open_intrrupt {
            unsafe { sstatus::set_sie() };
        }
    }
    pub fn leave_task_switch(&mut self, task: &mut LocalNow) {
        debug_assert!(!self.local_now.is_idle());
        debug_assert!(task.is_idle());
        // save floating register
        floating::switch_out(&mut self.task().thread.get_context().user_fx);
        self.task().thread.timer_leave_thread();
        let new = task.always(&mut self.always_local);
        let old = self.always();
        if old.sie_cur() == 0 {
            unsafe { sstatus::clear_sie() };
        }
        let open_intrrupt = AlwaysLocal::env_change(new, old);
        // 现在中断关闭了
        memory::set_satp_by_global();
        core::mem::swap(&mut self.local_now, task);
        if open_intrrupt {
            unsafe { sstatus::set_sie() };
        }
    }
    /// 进入内核线程会交换运行的`AlwaysLocal`和原来的`AlwaysLocal`, 退出时再切回来
    pub fn switch_kernel_task(&mut self, task: &mut AlwaysLocal) {
        // 进入内核线程不会改变 local_now, 因此它是调度态的
        debug_assert!(self.local_now.is_idle());
        let old = self.always();
        // 关中断 避免中断时使用always_local时发生错误
        if old.sie_cur() == 0 {
            unsafe { sstatus::clear_sie() };
        }
        let open_intrrupt = AlwaysLocal::env_change(task, old);
        // 现在中断关闭了
        self.always_local.swap(task);
        if open_intrrupt {
            unsafe { sstatus::set_sie() };
        }
    }
    pub fn enter_sleep(&mut self) {
        debug_assert!(!*self.sleep.get_mut());
        *self.sleep.get_mut() = true;
        executor::sleep_increase();
    }
    pub fn leave_sleep(&mut self) {
        // 如果被其他核唤醒了, sleep将是true
        if self
            .sleep
            .compare_exchange(true, false, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            executor::sleep_decrease();
        }
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
/// tp寄存器里面放的就是CPU控制块指针
///
/// 为什么要浪费tp这么大的64位寄存器只用来放一个数字?
///
/// tp放指针了怎么拿到CPU-ID? 把CPU-ID放控制块里面
#[inline(always)]
pub fn hart_local() -> &'static mut HartLocal {
    unsafe { &mut *(cpu::get_tp() as *mut HartLocal) }
}
///
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

#[inline]
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
pub fn all_hart_fn(mut f: impl FnMut(&mut HartMailBox)) {
    let hart_local = hart_local();
    let cur = hart_local.cpuid();
    unsafe {
        for local in cpu_local_in_use() {
            if local.cpuid() == cur {
                continue;
            }
            local.register(|m| f(m));
        }
    }
    f(&mut hart_local.local_mail);
    hart_local.local_mail.handle();
}

pub fn all_hart_fence_i() {
    all_hart_fn(|m| m.set_flag(MailEvent::FENCE_I));
}

pub fn all_hart_sfence_vma_asid(asid: Asid) {
    debug_assert!(USING_ASID || asid == Asid::ZERO);
    if USING_ASID {
        all_hart_fn(move |m| m.spec_sfence(None, Some(asid)))
    } else {
        all_hart_sfence_vma_all_no_global();
    }
}

pub fn all_hart_sfence_vma_va_asid(va: UserAddr4K, asid: Asid) {
    debug_assert!(USING_ASID || asid == Asid::ZERO);
    if USING_ASID {
        all_hart_fn(move |m| m.spec_sfence(Some(va), Some(asid)));
    } else {
        all_hart_sfence_vma_va_global(va);
    }
}

pub fn all_hart_sfence_vma_all_no_global() {
    assert!(!USING_ASID);
    all_hart_fn(|m| m.set_flag(MailEvent::SFENCE_VMA_ALL_NO_GLOBAL));
}

pub fn all_hart_sfence_vma_va_global(va: UserAddr4K) {
    all_hart_fn(move |m| m.spec_sfence(Some(va), None))
}

pub fn try_wake_sleep_hart() {
    if !executor::have_sleep() {
        return;
    }
    let this_cpu = hart_local().cpuid();
    unsafe {
        for cur in cpu_local_in_use().iter() {
            let cur_id = cur.cpuid();
            if cur_id == this_cpu {
                continue;
            }
            if cur.sleep.load(Ordering::Relaxed) {
                if cur
                    .sleep
                    .compare_exchange(true, false, Ordering::Relaxed, Ordering::Relaxed)
                    .is_err()
                {
                    continue;
                }
                executor::sleep_decrease();
                let r = sbi::send_ipi(1 << cur_id);
                assert_eq!(r, 0);
            }
        }
    }
}
