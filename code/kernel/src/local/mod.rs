use alloc::{boxed::Box, vec::Vec};

use crate::{
    config::PAGE_SIZE,
    hart::{self, cpu, sfence},
    memory::asid::Asid,
    sync::mutex::SpinNoIrqLock as Mutex,
    user::UserAccessStatus,
};

macro_rules! array_repeat {
    ($a: expr) => {
        [
            $a, $a, $a, $a, $a, $a, $a, $a, $a, $a, $a, $a, $a, $a, $a, $a,
        ]
    };
}

static mut HART_LOCAL: [Local; 16] = array_repeat!(Local::new());

pub struct Local {
    pub kstack_bottom: usize,
    pub user_access_status: UserAccessStatus,
    queue: Vec<Box<dyn FnOnce()>>,
    pending: Mutex<Vec<Box<dyn FnOnce()>>>,
}

impl Local {
    const fn new() -> Self {
        Self {
            kstack_bottom: 0,
            user_access_status: UserAccessStatus::Forbid,
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
}

#[inline(always)]
pub fn current_local() -> &'static mut Local {
    let i = cpu::hart_id();
    unsafe { &mut HART_LOCAL[i] }
}

fn get_local_by_id(id: usize) -> &'static Local {
    unsafe { &HART_LOCAL[id] }
}

pub fn set_stack() {
    let sp = hart::current_sp();
    current_local().kstack_bottom = (sp & !(PAGE_SIZE - 1)) + PAGE_SIZE;
}

#[inline(never)]
pub fn stack_size() -> usize {
    let sp = hart::current_sp();
    current_local().kstack_bottom - sp
}

pub fn handle_current_local() {
    current_local().handle()
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
