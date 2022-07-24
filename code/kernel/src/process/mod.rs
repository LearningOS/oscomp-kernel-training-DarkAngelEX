use alloc::{
    string::{String, ToString},
    sync::{Arc, Weak},
    vec::Vec,
};
use core::sync::atomic::{AtomicI32, AtomicUsize, Ordering};
use ftl_util::{
    error::SysR,
    fs::{Mode, OpenFlags},
};
use vfs::VfsFile;

use crate::{
    fs,
    memory::{asid::Asid, UserSpace},
    signal::manager::ProcSignalManager,
    sync::{even_bus::EventBus, mutex::SpinNoIrqLock as Mutex},
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
pub mod exit;
pub mod fd;
pub mod pid;
pub mod resource;
pub mod search;
pub mod thread;
pub mod tid;
pub mod userloop;
pub use {pid::Pid, tid::Tid};

bitflags! {
    pub struct CloneFlag: u64 {
        const EXIT_SIGNAL          =  0x000000ff;
        const CLONE_VM             =  0x00000100;
        const CLONE_FS             =  0x00000200;
        const CLONE_FILES          =  0x00000400;
        const CLONE_SIGHAND        =  0x00000800;
        const CLONE_PIDFD          =  0x00001000;
        const CLONE_PTRACE         =  0x00002000;
        const CLONE_VFORK          =  0x00004000;
        const CLONE_PARENT         =  0x00008000;
        const CLONE_THREAD         =  0x00010000;
        const CLONE_NEWNS          =  0x00020000;
        const CLONE_SYSVSEM        =  0x00040000;
        const CLONE_SETTLS         =  0x00080000;
        const CLONE_PARENT_SETTID  =  0x00100000;
        const CLONE_CHILD_CLEARTID =  0x00200000;
        const CLONE_DETACHED       =  0x00400000;
        const CLONE_UNTRACED       =  0x00800000;
        const CLONE_CHILD_SETTID   =  0x01000000;
        const CLONE_NEWCGROUP      =  0x02000000;
        const CLONE_NEWUTS         =  0x04000000;
        const CLONE_NEWIPC         =  0x08000000;
        const CLONE_NEWUSER        =  0x10000000;
        const CLONE_NEWPID         =  0x20000000;
        const CLONE_NEWNET         =  0x40000000;
        const CLONE_IO             =  0x80000000;
        const CLONE_CLEAR_SIGHAND  = 0x100000000;
        const CLONE_INTO_CGROUP    = 0x200000000;
    }
}

pub struct Process {
    pid: PidHandle,
    pub pgid: AtomicUsize,
    pub event_bus: Arc<EventBus>,
    pub signal_manager: ProcSignalManager,
    pub alive: Mutex<Option<AliveProcess>>,
    pub exit_code: AtomicI32,
}

impl Drop for Process {
    fn drop(&mut self) {
        search::clear_proc(self.pid());
    }
}

pub struct AliveProcess {
    pub user_space: UserSpace,
    pub cwd: Arc<VfsFile>,
    pub exec_path: String,
    pub envp: Vec<String>,
    pub parent: Option<Weak<Process>>, // assume upgrade success.
    pub children: ChildrenSet,
    pub threads: ThreadGroup,
    pub fd_table: FdTable,
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
    pub fn is_alive(&self) -> bool {
        unsafe { self.alive.unsafe_get().is_some() }
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
    pub fn fork(self: &Arc<Self>, new_pid: PidHandle) -> SysR<Arc<Self>> {
        let mut alive_guard = self.alive.lock();
        let alive = alive_guard.as_mut().unwrap();
        let user_space = alive.user_space.fork()?;
        let success_check = NeverFail::new();
        let new_alive = AliveProcess {
            user_space,
            cwd: alive.cwd.clone(),
            exec_path: alive.exec_path.clone(),
            envp: alive.envp.clone(),
            parent: Some(Arc::downgrade(self)),
            children: ChildrenSet::new(),
            threads: ThreadGroup::new(),
            fd_table: alive.fd_table.clone(),
        };
        let new_process = Arc::new(Process {
            pid: new_pid,
            pgid: AtomicUsize::new(self.pgid.load(Ordering::Relaxed)),
            event_bus: EventBus::new(),
            signal_manager: self.signal_manager.fork(),
            alive: Mutex::new(Some(new_alive)),
            exit_code: AtomicI32::new(i32::MIN),
        });
        alive.children.push_child(new_process.clone());
        success_check.assume_success();
        search::insert_proc(&new_process);
        Ok(new_process)
    }
}

impl AliveProcess {
    pub fn asid(&self) -> Asid {
        self.user_space.asid()
    }
    /// return: (parent, children)
    pub fn take_parent_children(&mut self) -> (Option<Weak<Process>>, ChildrenSet) {
        let parent = self.parent.take();
        let children = self.children.take();
        (parent, children)
    }
}

#[cfg(feature = "submit")]
static RUN_ALL_CASE: &'static [u8] = include_bytes!("../../run_all_case");
#[cfg(not(feature = "submit"))]
static RUN_ALL_CASE: &'static [u8] = &[];

pub async fn init() {
    let initproc = "/initproc";
    let cwd = fs::open_file((Err(SysError::ENOENT), "/"), OpenFlags::RDONLY, Mode(0o500))
        .await
        .unwrap();
    if cfg!(feature = "submit") {
        println!("running submit program!");
        let mut args = Vec::new();
        args.push(initproc.to_string());
        let envp = Vec::new();
        let thread = Thread::new_initproc(cwd, RUN_ALL_CASE, args, envp);
        userloop::spawn(thread);
    } else {
        println!("load initporc: {}", initproc);
        let inode = fs::open_file(
            (Err(SysError::ENOENT), initproc),
            OpenFlags::RDONLY,
            Mode(0o500),
        )
        .await
        .unwrap();
        let elf_data = inode.read_all().await.unwrap();
        let mut args = Vec::new();
        args.push(initproc.to_string());
        let envp = Vec::new();
        let thread = Thread::new_initproc(cwd, elf_data.as_slice(), args, envp);
        userloop::spawn(thread);
    }
    println!("spawn initporc completed");
}
