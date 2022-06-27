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
const SYSCALL_FCNTL: usize = 25;
const SYSCALL_IOCTL: usize = 29;
const SYSCALL_MKDIRAT: usize = 34;
const SYSCALL_UNLINKAT: usize = 35;
const SYSCALL_UMOUNT2: usize = 39;
const SYSCALL_MOUNT: usize = 40;
const SYSCALL_CHDIR: usize = 49;
const SYSCALL_OPENAT: usize = 56;
const SYSCALL_CLOSE: usize = 57;
const SYSCALL_PIPE: usize = 59;
const SYSCALL_GETDENTS64: usize = 61;
const SYSCALL_READ: usize = 63;
const SYSCALL_WRITE: usize = 64;
const SYSCALL_WRITEV: usize = 66;
const SYSCALL_PPOLL: usize = 73;
const SYSCALL_FSTAT: usize = 80;
const SYSCALL_EXIT: usize = 93;
const SYSCALL_EXIT_GROUP: usize = 94;
const SYSCALL_SET_TID_ADDRESS: usize = 96;
const SYSCALL_NANOSLEEP: usize = 101;
const SYSCALL_CLOCK_GETTIME: usize = 113;
const SYSCALL_SCHED_YIELD: usize = 124;
const SYSCALL_KILL: usize = 129;
const SYSCALL_RT_SIGSUSPEND: usize = 133;
const SYSCALL_RT_SIGACTION: usize = 134;
const SYSCALL_RT_SIGPROCMASK: usize = 135;
const SYSCALL_RT_SIGPENDING: usize = 136;
const SYSCALL_RT_SIGTIMEDWAIT: usize = 137;
const SYSCALL_RT_SIGQUEUEINFO: usize = 138;
const SYSCALL_RT_SIGRETURN: usize = 139;
const SYSCALL_TIMES: usize = 153;
const SYSCALL_SETPGID: usize = 154;
const SYSCALL_GETPGID: usize = 155;
const SYSCALL_UNAME: usize = 160;
const SYSCALL_GETTIMEOFDAY: usize = 169;
const SYSCALL_GETPID: usize = 172;
const SYSCALL_GETPPID: usize = 173;
const SYSCALL_GETUID: usize = 174;
const SYSCALL_GETEUID: usize = 175;
const SYSCALL_BRK: usize = 214;
const SYSCALL_MUNMAP: usize = 215;
const SYSCALL_CLONE: usize = 220;
const SYSCALL_EXECVE: usize = 221;
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
            SYSCALL_GETCWD => self.sys_getcwd().await,
            SYSCALL_DUP => self.sys_dup(),
            SYSCALL_DUP3 => self.sys_dup3(),
            SYSCALL_FCNTL => self.sys_fcntl(),
            SYSCALL_IOCTL => self.sys_ioctl(),
            SYSCALL_MKDIRAT => self.sys_mkdirat().await,
            SYSCALL_UNLINKAT => self.sys_unlinkat().await,
            SYSCALL_UMOUNT2 => self.sys_umount2().await,
            SYSCALL_MOUNT => self.sys_mount().await,
            SYSCALL_CHDIR => self.sys_chdir().await,
            SYSCALL_OPENAT => self.sys_openat().await,
            SYSCALL_CLOSE => self.sys_close(),
            SYSCALL_PIPE => self.sys_pipe().await,
            SYSCALL_GETDENTS64 => self.sys_getdents64().await,
            SYSCALL_READ => self.sys_read().await,
            SYSCALL_WRITE => self.sys_write().await,
            SYSCALL_WRITEV => self.sys_writev().await,
            SYSCALL_PPOLL => self.sys_ppoll().await,
            SYSCALL_FSTAT => self.sys_fstat().await,
            SYSCALL_EXIT => self.sys_exit(),
            SYSCALL_EXIT_GROUP => self.sys_exit_group(),
            SYSCALL_SET_TID_ADDRESS => self.sys_set_tid_address(),
            SYSCALL_NANOSLEEP => self.sys_nanosleep().await,
            SYSCALL_CLOCK_GETTIME => self.clock_gettime().await,
            SYSCALL_SCHED_YIELD => self.sys_sched_yield().await,
            SYSCALL_KILL => self.sys_kill(),
            SYSCALL_RT_SIGSUSPEND => self.sys_rt_sigsuspend().await,
            SYSCALL_RT_SIGACTION => self.sys_rt_sigaction().await,
            SYSCALL_RT_SIGPROCMASK => self.sys_rt_sigprocmask().await,
            SYSCALL_RT_SIGPENDING => self.sys_rt_sigpending().await,
            SYSCALL_RT_SIGTIMEDWAIT => self.sys_rt_sigtimedwait().await,
            SYSCALL_RT_SIGQUEUEINFO => self.sys_rt_sigqueueinfo().await,
            SYSCALL_RT_SIGRETURN => self.sys_rt_sigreturn().await,
            SYSCALL_TIMES => self.sys_times().await,
            SYSCALL_SETPGID => self.sys_setpgid(),
            SYSCALL_GETPGID => self.sys_getpgid(),
            SYSCALL_UNAME => self.sys_uname().await,
            SYSCALL_GETTIMEOFDAY => self.sys_gettimeofday().await,
            SYSCALL_GETPID => self.sys_getpid(),
            SYSCALL_GETPPID => self.sys_getppid(),
            SYSCALL_GETUID => self.sys_getuid(),
            SYSCALL_GETEUID => self.sys_geteuid(),
            SYSCALL_BRK => self.sys_brk(),
            SYSCALL_MUNMAP => self.sys_munmap(),
            SYSCALL_CLONE => self.sys_clone(),
            SYSCALL_EXECVE => self.sys_execve().await,
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
            // println!("syscall return with {}", a0);
            if ![63, 64].contains(&self.cx.a7()) {
                println!("syscall {} -> {:#x}", self.cx.a7(), a0);
            }
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
        let lock = self.process.alive.lock();
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
