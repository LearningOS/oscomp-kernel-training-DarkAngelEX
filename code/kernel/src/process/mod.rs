use alloc::{
    string::String,
    sync::{Arc, Weak},
};
use core::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

use crate::{
    loader,
    memory::{
        allocator::frame::{self, FrameAllocator},
        UserSpace,
    },
    sync::{even_bus::EventBus, mutex::SpinNoIrqLock as Mutex},
    tools::error::FrameOutOfMemory,
    xdebug::NeverFail,
};

use self::{
    children::ChildrenSet,
    pid::{pid_alloc, PidHandle},
    thread::{Thread, ThreadGroup},
};

pub mod children;
pub mod pid;
pub mod thread;
pub mod tid;
pub mod userloop;
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
    pub parent: Option<Weak<Process>>, // assume upgrade success.
    pub children: ChildrenSet,
    pub threads: ThreadGroup,
}

impl Process {
    pub fn pid(&self) -> Pid {
        self.pid.pid()
    }
    // return Err if zombies
    pub fn alive_then<T>(&self, f: impl FnOnce(&mut AliveProcess) -> T) -> Result<T, ()> {
        match self.alive.lock(place!()).as_mut() {
            Some(alive) => Ok(f(alive)),
            None => Err(()),
        }
    }
    pub fn using_space(&self) {
        self.alive_then(|a| a.user_space.using()).unwrap();
    }

    // fork and release all thread except tid
    pub fn fork(
        self: &Arc<Self>,
        tid: Tid,
        allocator: &mut impl FrameAllocator,
    ) -> Result<Arc<Self>, FrameOutOfMemory> {
        let mut alive_guard = self.alive.lock(place!());
        let alive = alive_guard.as_mut().unwrap();
        let mut user_space = alive.user_space.fork(allocator)?;
        let stack_id = alive.threads.map(tid).unwrap().inner().stack_id;
        unsafe {
            user_space.stack_dealloc_all_except(stack_id, allocator);
        }
        let success_check = NeverFail::new();
        let new_pid = pid_alloc();
        let new_alive = AliveProcess {
            user_space,
            cwd: alive.cwd.clone(),
            exec_path: alive.exec_path.clone(),
            parent: Some(Arc::downgrade(self)),
            children: ChildrenSet::new(),
            threads: ThreadGroup::new(),
        };
        let new_process = Arc::new(Process {
            pid: new_pid,
            pgid: AtomicUsize::new(self.pgid.load(Ordering::Relaxed)),
            event_bus: EventBus::new(),
            alive: Mutex::new(Some(new_alive)),
            exit_code: AtomicI32::new(0),
        });
        alive.children.push_child(new_process.clone());
        success_check.assume_success();
        Ok(new_process)
    }
}

pub fn init() {
    println!("load initporc");
    let allocator = &mut frame::defualt_allocator();
    let elf_data = loader::get_app_data_by_name("initproc").unwrap();
    let thread = Thread::new(elf_data, allocator);
    userloop::spawn(thread);
    println!("spawn initporc");
}
