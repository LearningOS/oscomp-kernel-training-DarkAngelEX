use alloc::{boxed::Box, vec::Vec};
use riscv::register::sstatus;

use crate::{
    config::PAGE_SIZE,
    hart::{self, cpu, sfence},
    memory::asid::{Asid, AsidVersion},
    sync::mutex::SpinNoIrqLock as Mutex,
};

use self::task_local::TaskLocal;

pub mod task_local;

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
    task_local: Option<Box<TaskLocal>>,
    queue: Vec<Box<dyn FnOnce()>>,
    pending: Mutex<Vec<Box<dyn FnOnce()>>>,
    kstack_bottom: usize,
    asid_version: AsidVersion,
    pub in_exception: bool, // forbid exception nest
}
impl HartLocal {
    const fn new() -> Self {
        Self {
            task_local: None,
            queue: Vec::new(),
            pending: Mutex::new(Vec::new()),
            kstack_bottom: 0,
            asid_version: AsidVersion::first_asid_version(),
            in_exception: false,
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
        self.task_local.as_mut().map(|a| a.as_mut())
    }
    pub fn task(&mut self) -> &mut TaskLocal {
        self.task_local.as_mut().unwrap().as_mut()
    }
    pub fn set_task(&mut self, task: Box<TaskLocal>) {
        task.set_env();
        assert!(self.task_local.is_none());
        self.task_local = Some(task);
    }
    pub fn take_task(&mut self) -> Box<TaskLocal> {
        let ret = self.task_local.take().unwrap();
        ret.clear_env();
        ret
    }
    pub fn sstatus_zero_assert(&self) {
        let sstatus = sstatus::read();
        assert!(!sstatus.sum());
        assert!(!sstatus.sie());
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
    // ceil 4KB
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
    all_hart_fn(|| || sfence::fence_i());
}

pub fn all_hart_sfence_vma_asid(asid: Asid) {
    all_hart_fn(|| move || sfence::sfence_vma_asid(asid.into_usize()));
}
