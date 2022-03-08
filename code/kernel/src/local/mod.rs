use alloc::{boxed::Box, sync::Arc, vec::Vec};
use riscv::register::sstatus;

use crate::{
    config::PAGE_SIZE,
    hart::{self, cpu, sfence},
    memory::{
        asid::{Asid, AsidVersion},
        PageTable,
    },
    process::{thread::Thread, Tid},
    sync::mutex::SpinNoIrqLock as Mutex,
    tools::container::sync_unsafe_cell::SyncUnsafeCell,
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
    pub thread: Option<Arc<Thread>>,
    pub page_table: Option<Arc<SyncUnsafeCell<PageTable>>>,
    sie_count: usize,
    sum_count: usize,
    asid_version: AsidVersion,
    queue: Vec<Box<dyn FnOnce()>>,
    pending: Mutex<Vec<Box<dyn FnOnce()>>>,
}

impl Local {
    const fn new() -> Self {
        Self {
            kstack_bottom: 0,
            user_access_status: UserAccessStatus::Forbid,
            thread: None,
            page_table: None,
            sie_count: 0,
            sum_count: 0,
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
    pub fn null_owner_assert(&self) {
        assert!(self.thread.is_none());
        assert!(self.page_table.is_none());
    }
    pub fn have_owner_assert(&self, tid: Tid) {
        assert_eq!(self.thread.as_ref().unwrap().tid, tid);
        assert!(self.page_table.is_some());
    }
    pub fn sstatus_zero_assert(&self) {
        assert!(self.sum_count == 0);
        assert!(self.sie_count == 0);
        let sstatus = sstatus::read();
        assert!(!sstatus.sum());
        assert!(!sstatus.sie());
    }
    pub fn sum_inc(&mut self) {
        assert!(self.sie_count != 0);
        if self.sum_count == 0 {
            unsafe { sstatus::set_sum() };
        }
        self.sum_count += 1;
    }
    pub fn sum_dec(&mut self) {
        assert!(self.sie_count != 0);
        assert!(self.sum_count != 0);
        self.sum_count -= 1;
        if self.sum_count == 0 {
            unsafe { sstatus::clear_sum() };
        }
    }
    pub fn sie_inc(&mut self) {
        if self.sie_count == 0 {
            unsafe { sstatus::set_sie() };
        }
        self.sie_count += 1;
    }
    pub fn sie_dec(&mut self) {
        assert!(self.sie_count != 0);
        self.sie_count -= 1;
        if self.sie_count == 0 {
            unsafe { sstatus::clear_sie() };
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

pub fn asid_version_update(latest_version: AsidVersion) {
    let cpu_version = &mut current_local().asid_version;
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
