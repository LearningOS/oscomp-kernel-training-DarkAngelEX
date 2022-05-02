use core::ops::{Deref, DerefMut};

use alloc::sync::Arc;

use crate::{
    process::{thread::Thread, AliveProcess, Process},
    trap::context::UKContext,
    xdebug::PRINT_SYSCALL_ALL,
};

mod fs;
mod mmap;
mod process;
mod signal;
mod thread;
mod time;

pub use ftl_util::error::{SysError, UniqueSysError};

const SYSCALL_GETCWD: usize = 17;
const SYSCALL_DUP: usize = 23;
const SYSCALL_DUP3: usize = 24;
const SYSCALL_IOCTL: usize = 29;
const SYSCALL_OPENAT: usize = 56;
const SYSCALL_CLOSE: usize = 57;
const SYSCALL_PIPE: usize = 59;
const SYSCALL_READ: usize = 63;
const SYSCALL_WRITE: usize = 64;
const SYSCALL_EXIT: usize = 93;
const SYSCALL_EXIT_GROUP: usize = 94;
const SYSCALL_SET_TID_ADDRESS: usize = 96;
const SYSCALL_SLEEP: usize = 101;
const SYSCALL_YIELD: usize = 124;
const SYSCALL_KILL: usize = 129;
const SYSCALL_RT_SIGACTION: usize = 134;
const SYSCALL_RT_SIGPROCMASK: usize = 135;
const SYSCALL_GET_TIME: usize = 169;
const SYSCALL_GETPID: usize = 172;
const SYSCALL_GETUID: usize = 174;
const SYSCALL_BRK: usize = 214;
const SYSCALL_CLONE: usize = 220;
const SYSCALL_EXEC: usize = 221;
const SYSCALL_MMAP: usize = 222;
const SYSCALL_MPROTECT: usize = 226;
const SYSCALL_WAIT4: usize = 260;

// rCore-tutorial
const SYSCALL_THREAD_CREATE: usize = 1000;
const SYSCALL_GETTID: usize = 1001;
const SYSCALL_WAITTID: usize = 1002;
const SYSCALL_MUTEX_CREATE: usize = 1010;
const SYSCALL_MUTEX_LOCK: usize = 1011;
const SYSCALL_MUTEX_UNLOCK: usize = 1012;
const SYSCALL_SEMAPHORE_CREATE: usize = 1020;
const SYSCALL_SEMAPHORE_UP: usize = 1021;
const SYSCALL_SEMAPHORE_DOWN: usize = 1022;
const SYSCALL_CONDVAR_CREATE: usize = 1030;
const SYSCALL_CONDVAR_SIGNAL: usize = 1031;
const SYSCALL_CONDVAR_WAIT: usize = 1032;

pub struct Syscall<'a> {
    cx: &'a mut UKContext,
    thread: &'a Thread,
    process: &'a Process,
    do_exit: bool,
}

impl<'a> Syscall<'a> {
    pub fn new(
        cx: &'a mut UKContext,
        thread_arc: &'a Arc<Thread>,
        process_arc: &'a Arc<Process>,
    ) -> Self {
        Self {
            cx,
            thread: thread_arc.as_ref(),
            // thread_arc,
            process: process_arc.as_ref(),
            // process_arc,
            do_exit: false,
        }
    }
    /// return do_exit
    #[inline(always)]
    pub async fn syscall(&mut self) -> bool {
        stack_trace!();
        self.cx.set_next_instruction();
        let result: SysResult = match self.cx.a7() {
            SYSCALL_GETCWD => self.getcwd().await,
            SYSCALL_DUP => self.sys_dup(),
            SYSCALL_DUP3 => self.sys_dup3(),
            SYSCALL_IOCTL => self.sys_ioctl(),
            SYSCALL_OPENAT => self.sys_openat().await,
            SYSCALL_CLOSE => self.sys_close(),
            SYSCALL_PIPE => self.sys_pipe().await,
            SYSCALL_READ => self.sys_read().await,
            SYSCALL_WRITE => self.sys_write().await,
            SYSCALL_EXIT => self.sys_exit(),
            SYSCALL_EXIT_GROUP => self.sys_exit_group(),
            SYSCALL_SET_TID_ADDRESS => self.sys_set_tid_address(),
            SYSCALL_SLEEP => self.sys_sleep().await,
            SYSCALL_YIELD => self.sys_yield().await,
            SYSCALL_KILL => self.sys_kill(),
            SYSCALL_RT_SIGACTION => self.sys_rt_sigaction().await,
            SYSCALL_RT_SIGPROCMASK => self.sys_rt_sigprocmask().await,
            SYSCALL_GET_TIME => self.sys_gettime(),
            SYSCALL_GETPID => self.sys_getpid(),
            SYSCALL_GETUID => self.sys_getuid(),
            SYSCALL_BRK => self.sys_brk(),
            SYSCALL_CLONE => self.sys_clone(),
            SYSCALL_EXEC => self.sys_exec().await,
            SYSCALL_MMAP => self.sys_mmap(),
            SYSCALL_MPROTECT => self.sys_mprotect(),
            SYSCALL_WAIT4 => self.sys_wait4().await,

            SYSCALL_THREAD_CREATE => self.sys_thread_create(),
            SYSCALL_GETTID => todo!(),
            SYSCALL_WAITTID => todo!(),
            SYSCALL_MUTEX_CREATE => todo!(),
            SYSCALL_MUTEX_LOCK => todo!(),
            SYSCALL_MUTEX_UNLOCK => todo!(),
            SYSCALL_SEMAPHORE_CREATE => todo!(),
            SYSCALL_SEMAPHORE_UP => todo!(),
            SYSCALL_SEMAPHORE_DOWN => todo!(),
            SYSCALL_CONDVAR_CREATE => todo!(),
            SYSCALL_CONDVAR_SIGNAL => todo!(),
            SYSCALL_CONDVAR_WAIT => todo!(),
            unknown => panic!("[kernel]unsupported syscall_id: {}", unknown),
        };
        let a0 = match result {
            Ok(a) => a,
            // Err(e) => -(e as isize) as usize,
            Err(_e) => -1isize as usize,
        };
        memory_trace!("syscall return");
        if PRINT_SYSCALL_ALL {
            println!("syscall return with {}", a0);
        }
        self.cx.set_user_a0(a0);
        self.do_exit
    }
    /// if return Err will set do_exit = true
    #[inline(always)]
    pub fn alive_then<T>(
        &mut self,
        f: impl FnOnce(&mut AliveProcess) -> T,
    ) -> Result<T, UniqueSysError<{ SysError::ESRCH as isize }>> {
        self.process
            .alive_then(f)
            .inspect_err(|_e| self.do_exit = true)
            .map_err(From::from)
    }
    /// if return Err will set do_exit = true
    #[inline(always)]
    pub fn alive_lock(
        &mut self,
    ) -> Result<
        impl DerefMut<Target = AliveProcess> + 'a,
        UniqueSysError<{ SysError::ESRCH as isize }>,
    > {
        let lock = self.process.alive.lock(place!());
        if lock.is_none() {
            self.do_exit = true;
            return Err(UniqueSysError);
        }
        return Ok(AliveGurad(lock));
    }
}

trait DAP = DerefMut<Target = Option<AliveProcess>>;
struct AliveGurad<M: DAP>(M);
impl<M: DAP> Deref for AliveGurad<M> {
    type Target = AliveProcess;
    fn deref(&self) -> &Self::Target {
        unsafe { self.0.as_ref().unwrap_unchecked() }
    }
}
impl<M: DAP> DerefMut for AliveGurad<M> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.0.as_mut().unwrap_unchecked() }
    }
}

pub type SysResult = Result<usize, SysError>;
