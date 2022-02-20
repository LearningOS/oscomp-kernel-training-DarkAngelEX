use alloc::{
    string::String,
    sync::{Arc, Weak},
    vec::Vec,
};
use core::sync::atomic::{AtomicI32, AtomicUsize};

use crate::{
    memory::UserSpace,
    sync::{even_bus::EventBus, mutex::SpinNoIrqLock as Mutex},
};

use self::{pid::PidHandle, thread::Thread};

pub mod children;
mod userloop;
pub mod pid;
pub mod thread;
pub mod tid;
pub use {pid::Pid, tid::Tid};

pub struct Process {
    pid: PidHandle,
    pub pgid: AtomicUsize,
    pub event_bus: Arc<Mutex<EventBus>>,
    pub alive: Mutex<Option<AliveProcess>>,
    pub exit_code: AtomicI32,
}

pub struct AliveProcess {
    pub user_space: UserSpace,
    pub cwd: String,
    pub exec_path: String,
    pub parent: Option<Weak<Process>>,
    pub children: Vec<Arc<Process>>,
    pub threads: Vec<Weak<Thread>>,
}

impl Process {
    pub fn pid(&self) -> Pid {
        self.pid.pid()
    }
    pub fn alive_then<T>(&self, f: impl FnOnce(&mut AliveProcess) -> T) -> Result<T, ()> {
        match self.alive.lock(place!()).as_mut() {
            Some(alive) => Ok(f(alive)),
            None => Err(()),
        }
    }
}

pub fn init() {
    println!("load initporc");

}