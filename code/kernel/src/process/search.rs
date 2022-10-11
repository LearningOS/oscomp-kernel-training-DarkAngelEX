use core::cell::OnceCell;
use alloc::{
    collections::BTreeMap,
    sync::{Arc, Weak},
};

use crate::sync::mutex::SpinNoIrqLock as Mutex;

use super::{thread::Thread, Pid, Process, Tid};

static PROC_MAP: Mutex<BTreeMap<Pid, Weak<Process>>> = Mutex::new(BTreeMap::new());
static THREAD_MAP: Mutex<BTreeMap<Tid, Weak<Thread>>> = Mutex::new(BTreeMap::new());
static mut INITPROC: OnceCell<Arc<Process>> = OnceCell::new();

pub fn proc_count() -> usize {
    unsafe { PROC_MAP.unsafe_get().len() }
}

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

pub fn find_thread(tid: Tid) -> Option<Arc<Thread>> {
    THREAD_MAP.lock().get_mut(&tid)?.upgrade()
}

pub fn insert_thread(thread: &Arc<Thread>) {
    THREAD_MAP
        .lock()
        .insert(thread.tid(), Arc::downgrade(thread));
}

pub fn clear_thread(tid: Tid) {
    THREAD_MAP.lock().remove(&tid).unwrap();
}
