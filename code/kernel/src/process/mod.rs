use alloc::{
    collections::LinkedList,
    string::{String, ToString},
    sync::{Arc, Weak},
    vec::Vec,
};
use core::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

use crate::{
    fs::{self, Mode},
    memory::{asid::Asid, UserSpace},
    signal::SignalPack,
    sync::{
        even_bus::{Event, EventBus},
        mutex::SpinNoIrqLock as Mutex,
    },
    syscall::{SysError, UniqueSysError},
    xdebug::NeverFail,
};

use self::{
    children::ChildrenSet,
    fd::FdTable,
    pid::PidHandle,
    thread::{Thread, ThreadGroup},
};

pub mod children;
pub mod fd;
pub mod pid;
pub mod proc_table;
pub mod thread;
pub mod tid;
pub mod userloop;
pub use {pid::Pid, tid::Tid};

bitflags! {
    pub struct CloneFlag: u32 {
        const SIGCHLD = 17;
        const CLONE_CHILD_CLEARTID = 0x00200000;
        const CLONE_CHILD_SETTID = 0x01000000;
    }
}

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
    pub envp: Vec<String>,
    pub parent: Option<Weak<Process>>, // assume upgrade success.
    pub children: ChildrenSet,
    pub threads: ThreadGroup,
    pub fd_table: FdTable,
    pub signal_queue: LinkedList<SignalPack>,
}

#[derive(Debug)]
pub struct Dead;

impl From<Dead> for UniqueSysError<{ SysError::ESRCH as isize }> {
    fn from(_e: Dead) -> Self {
        UniqueSysError
    }
}

impl From<Dead> for SysError {
    fn from(e: Dead) -> Self {
        let err: UniqueSysError<_> = e.into();
        err.into()
    }
}

impl Process {
    pub fn pid(&self) -> Pid {
        self.pid.pid()
    }
    // return Err if zombies
    #[inline(always)]
    pub fn alive_then<T>(&self, f: impl FnOnce(&mut AliveProcess) -> T) -> Result<T, Dead> {
        match self.alive.lock().as_mut() {
            Some(alive) => Ok(f(alive)),
            None => Err(Dead),
        }
    }
    // fork and release all thread except tid
    pub fn fork(self: &Arc<Self>, tid: Tid) -> Result<Arc<Self>, SysError> {
        let mut alive_guard = self.alive.lock();
        let alive = alive_guard.as_mut().unwrap();
        let mut user_space = alive.user_space.fork()?;
        let stack_id = alive.threads.map(tid).unwrap().inner().stack_id;
        unsafe {
            user_space.stack_dealloc_all_except(stack_id);
        }
        let success_check = NeverFail::new();
        let new_pid = pid::pid_alloc();
        let new_alive = AliveProcess {
            user_space,
            cwd: alive.cwd.clone(),
            exec_path: alive.exec_path.clone(),
            envp: alive.envp.clone(),
            parent: Some(Arc::downgrade(self)),
            children: ChildrenSet::new(),
            threads: ThreadGroup::new(new_pid.get_usize() + 1),
            fd_table: alive.fd_table.clone(),
            signal_queue: LinkedList::new(),
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
    pub fn asid(&self) -> Asid {
        self.user_space.asid()
    }
    // return parent
    pub fn clear_all(&mut self, pid: Pid) {
        let this_parent = self.parent.take().and_then(|p| p.upgrade());
        let mut this_parent_alive = this_parent.as_ref().map(|p| (&p.event_bus, p.alive.lock()));
        let bus = match &mut this_parent_alive {
            Some((bus, ref mut p)) if p.is_some() => {
                // println!("origin's zombie, move:");
                // self.children.show();
                let p = p.as_mut().unwrap();
                p.children.become_zombie(pid);
                bus.clone()
            }
            _ => {
                // println!("initproc's zombie");
                let initproc = proc_table::get_initproc();
                let mut initproc_alive = initproc.alive.lock();
                let p = initproc_alive.as_mut().unwrap();
                p.children.become_zombie(pid);
                initproc.event_bus.clone()
            }
        };
        drop(this_parent_alive);
        let _ = bus.as_ref().lock().set(Event::CHILD_PROCESS_QUIT);
        if !self.children.is_empty() {
            let initproc = proc_table::get_initproc();
            let mut initproc_alive = initproc.alive.lock();
            let ich = &mut initproc_alive.as_mut().unwrap().children;
            ich.append(self.children.take());
            if ich.have_zombies() {
                drop(initproc_alive);
                let _ = initproc.event_bus.lock().set(Event::CHILD_PROCESS_QUIT);
            }
        }
    }
}

static RUN_ALL_CASE: &'static [u8] = include_bytes!("../../run_all_case");

pub async fn init() {
    let initproc = "/initproc";
    if cfg!(feature = "submit") || true {
        let mut args = Vec::new();
        args.push(initproc.to_string());
        let envp = Vec::new();
        let thread = Thread::new_initproc(RUN_ALL_CASE, args, envp);
        userloop::spawn(thread);
    } else {
        println!("load initporc: {}", initproc);
        let inode = fs::open_file("", initproc, fs::OpenFlags::RDONLY, Mode(0o500))
            .await
            .unwrap();
        let elf_data = inode.read_all().await.unwrap();
        let mut args = Vec::new();
        args.push(initproc.to_string());
        let envp = Vec::new();
        let thread = Thread::new_initproc(elf_data.as_slice(), args, envp);
        userloop::spawn(thread);
    }
    println!("spawn initporc");
}
