use core::ptr::NonNull;

use alloc::{boxed::Box, vec::Vec};
use riscv::register::sstatus;

use crate::{
    config::PAGE_SIZE,
    hart::{self, cpu, sfence},
    memory::{
        self,
        allocator::LocalHeap,
        asid::{Asid, AsidVersion},
    },
    sync::mutex::SpinNoIrqLock as Mutex,
};

use self::{always_local::AlwaysLocal, task_local::TaskLocal};

pub mod always_local;
pub mod task_local;

#[allow(clippy::declare_interior_mutable_const)]
const HART_LOCAL_EACH: HartLocal = HartLocal::new();
static mut HART_LOCAL: [HartLocal; 16] = [HART_LOCAL_EACH; 16];

#[allow(clippy::declare_interior_mutable_const)]
const ALWAYS_LOCAL_EACH: AlwaysLocal = AlwaysLocal::new();
static mut ALWAYS_LOCAL: [AlwaysLocal; 16] = [ALWAYS_LOCAL_EACH; 16];

/// any hart can only access each unit so didn't need mutex.
///
/// access other local must through function below.
pub struct HartLocal {
    local_now: LocalNow,
    queue: Vec<Box<dyn FnOnce()>>,
    pending: Mutex<Vec<Box<dyn FnOnce()>>>,
    kstack_bottom: usize,
    asid_version: AsidVersion,
    pub in_exception: bool, // forbid exception nest
    pub local_heap: LocalHeap,
}

pub enum LocalNow {
    Idle(Option<NonNull<AlwaysLocal>>),
    Task(Box<TaskLocal>),
}

unsafe impl Send for LocalNow {}
unsafe impl Sync for LocalNow {}

impl LocalNow {
    #[inline(always)]
    pub fn always(&mut self) -> &mut AlwaysLocal {
        match self {
            LocalNow::Idle(i) => unsafe { i.unwrap().as_mut() },
            LocalNow::Task(t) => t.always(),
        }
    }
    #[inline(always)]
    pub fn task(&mut self) -> &mut TaskLocal {
        match self {
            LocalNow::Idle(_i) => panic!(),
            LocalNow::Task(task) => task.as_mut(),
        }
    }
}

impl HartLocal {
    const fn new() -> Self {
        Self {
            local_now: LocalNow::Idle(None),
            queue: Vec::new(),
            pending: Mutex::new(Vec::new()),
            kstack_bottom: 0,
            asid_version: AsidVersion::first_asid_version(),
            in_exception: false,
            local_heap: LocalHeap::new(),
        }
    }
    /// must init after init memory.
    fn init(&mut self) {
        let idle = match &mut self.local_now {
            LocalNow::Idle(idle) => idle,
            LocalNow::Task(_task) => panic!(),
        };
        assert!(idle.is_none());
        // *idle = Some(Box::new(AlwaysLocal::new()));
        // ALWAYS_LOCAL 没有和分配器对齐! 此部分禁止释放到全局分配器
        let hart = cpu::hart_id();
        unsafe {
            *idle = NonNull::new(&mut ALWAYS_LOCAL[hart]);
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
    #[inline(always)]
    pub fn task(&mut self) -> &mut TaskLocal {
        self.local_now.task()
    }
    #[inline(always)]
    pub fn always(&mut self) -> &mut AlwaysLocal {
        self.local_now.always()
    }
    pub fn enter_task_switch(&mut self, task: &mut LocalNow) {
        assert!(matches!(&mut self.local_now, LocalNow::Idle(_)));
        assert!(matches!(task, LocalNow::Task(_)));
        let old = self.always();
        let new = task.always();
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
        assert!(matches!(task, LocalNow::Idle(_)));
        let old = self.always();
        let new = task.always();
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
}
pub fn init() {
    hart_local().init()
}
#[inline(always)]
pub fn hart_local() -> &'static mut HartLocal {
    let i = cpu::hart_id();
    unsafe { &mut HART_LOCAL[i] }
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
fn get_local_by_id(id: usize) -> &'static HartLocal {
    unsafe { &HART_LOCAL[id] }
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
    for i in 0..cpu::count() {
        if i == cur {
            continue;
        }
        get_local_by_id(i).register(f());
    }
    f()();
}

pub fn all_hart_fence_i() {
    all_hart_fn(|| sfence::fence_i);
}

pub fn all_hart_sfence_vma_asid(asid: Asid) {
    all_hart_fn(|| move || sfence::sfence_vma_asid(asid.into_usize()));
}
