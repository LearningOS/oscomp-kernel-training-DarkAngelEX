use alloc::{
    string::String,
    sync::{Arc, Weak},
};
use core::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

use crate::{
    loader,
    memory::{
        allocator::frame::{self, FrameAllocator},
        StackID, UserSpace,
    },
    sync::{
        even_bus::{self, Event, EventBus},
        mutex::SpinNoIrqLock as Mutex,
    },
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
pub mod proc_table;
pub mod thread;
pub mod tid;
pub mod userloop;
pub use {pid::Pid, tid::Tid};

pub struct Process {
    pid: PidHandle,
    pub pgid: AtomicUsize,
    /// if need to lock bus and alive at the same time,
    /// must lock alive first, then lock bus.
    pub event_bus: Arc<Mutex<EventBus>>,
    pub alive: Mutex<Option<AliveProcess>>,
    pub exit_code: AtomicI32,
}

impl Drop for Process {
    fn drop(&mut self) {
        proc_table::clear_proc(self.pid());
    }
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
        proc_table::insert_proc(&new_process);
        Ok(new_process)
    }
}

impl AliveProcess {
    // return parent
    pub fn clear_all(&mut self, pid: Pid) {
        let (bus, have_zombies) = {
            let this_parent = self.parent.take().and_then(|p| p.upgrade());
            let mut this_alive = this_parent
                .as_ref()
                .map(|p| (&p.event_bus, p.alive.lock(place!())));
            let initproc;
            let mut initproc_alive;
            let (bus, alive) = match &mut this_alive {
                Some((bus, ref mut p)) if p.is_some() => {
                    let p = p.as_mut().unwrap();
                    p.children.become_zombie(pid);
                    (*bus, p)
                }
                _ => {
                    initproc = proc_table::get_initproc();
                    initproc_alive = initproc.alive.lock(place!());
                    let p = initproc_alive.as_mut().unwrap();
                    p.children.become_zombie(pid);
                    (&initproc.event_bus, p)
                }
            };
            alive.children.append(self.children.take());
            (bus.clone(), alive.children.have_zombies())
        };
        if have_zombies {
            // ignore error.
            let _ = bus.lock(place!()).set(Event::CHILD_PROCESS_QUIT);
        }
    }
    pub fn dealloc_thread(&mut self, tid: Tid, stack_id: StackID) {
        todo!()
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
