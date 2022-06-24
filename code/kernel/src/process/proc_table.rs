use core::lazy::OnceCell;

use alloc::{
    collections::BTreeMap,
    sync::{Arc, Weak},
};

use crate::sync::mutex::SpinNoIrqLock as Mutex;

use super::{Pid, Process};

static PROC_MAP: Mutex<BTreeMap<Pid, Weak<Process>>> = Mutex::new(BTreeMap::new());
static mut INITPROC: OnceCell<Arc<Process>> = OnceCell::new();

/// 由于使用弱指针，智能指针开销不可忽略
pub fn find_proc(pid: Pid) -> Option<Arc<Process>> {
    PROC_MAP.lock().get_mut(&pid)?.upgrade()
}

pub fn insert_proc(proc: &Arc<Process>) {
    PROC_MAP.lock().insert(proc.pid(), Arc::downgrade(proc));
}

pub fn clear_proc(pid: Pid) {
    PROC_MAP.lock().remove(&pid).unwrap();
}

pub unsafe fn set_initproc(p: Arc<Process>) {
    INITPROC
        .set(p)
        .unwrap_or_else(|_e| panic!("initproc double set"))
}

pub fn get_initproc() -> Arc<Process> {
    unsafe { INITPROC.get().unwrap().clone() }
}
