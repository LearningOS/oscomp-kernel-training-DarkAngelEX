use alloc::{
    collections::BTreeMap,
    sync::{Arc, Weak},
};

use crate::{
    sync::mutex::SpinNoIrqLock as Mutex, tools::allocator::from_usize_allocator::FromUsize,
};

use super::{Pid, Process};

static PROC_MAP: Mutex<BTreeMap<Pid, Weak<Process>>> = Mutex::new(BTreeMap::new());

pub fn map(pid: Pid) -> Option<Arc<Process>> {
    PROC_MAP.lock(place!()).get_mut(&pid)?.upgrade()
}

pub fn get_proc(pid: Pid) -> Option<Arc<Process>> {
    PROC_MAP.lock(place!()).get_mut(&pid)?.upgrade()
}

pub fn insert_proc(proc: &Arc<Process>) {
    PROC_MAP
        .lock(place!())
        .insert(proc.pid(), Arc::downgrade(&proc));
}

pub fn clear_proc(pid: Pid) {
    PROC_MAP.lock(place!()).remove(&pid).unwrap();
}

pub fn get_initproc() -> Arc<Process> {
    get_proc(Pid::from_usize(0)).unwrap()
}
