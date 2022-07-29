use core::ops::{Deref, DerefMut};

use alloc::sync::Arc;
use ftl_util::error::SysRet;

use crate::{
    process::{thread::Thread, AliveProcess, Process},
    trap::context::UKContext,
    xdebug::PRINT_SYSCALL_ALL,
};

mod fs;
mod futex;
mod mmap;
mod net;
mod process;
mod random;
mod resource;
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
const SYSCALL_STATFS: usize = 43;
const SYSCALL_CHDIR: usize = 49;
const SYSCALL_OPENAT: usize = 56;
const SYSCALL_CLOSE: usize = 57;
const SYSCALL_PIPE2: usize = 59;
const SYSCALL_GETDENTS64: usize = 61;
const SYSCALL_LSEEK: usize = 62;
const SYSCALL_READ: usize = 63;
const SYSCALL_WRITE: usize = 64;
const SYSCALL_READV: usize = 65;
const SYSCALL_WRITEV: usize = 66;
const SYSCALL_PREAD64: usize = 67;
const SYSCALL_PPOLL: usize = 73;
const SYSCALL_READLINKAT: usize = 78;
const SYSCALL_NEWFSTATAT: usize = 79;
const SYSCALL_FSTAT: usize = 80;
const SYSCALL_UTIMENSAT: usize = 88;
const SYSCALL_EXIT: usize = 93;
const SYSCALL_EXIT_GROUP: usize = 94;
const SYSCALL_SET_TID_ADDRESS: usize = 96;
const SYSCALL_FUTEX: usize = 98;
const SYSCALL_SET_ROBUST_LIST: usize = 99;
const SYSCALL_GET_ROBUST_LIST: usize = 100;
const SYSCALL_NANOSLEEP: usize = 101;
const SYSCALL_CLOCK_GETTIME: usize = 113;
const SYSCALL_SCHED_YIELD: usize = 124;
const SYSCALL_KILL: usize = 129;
const SYSCALL_TKILL: usize = 130;
const SYSCALL_TGKILL: usize = 131;
const SYSCALL_SIGALTSTACK: usize = 132;
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
const SYSCALL_GETRUSAGE: usize = 165;
const SYSCALL_GETTIMEOFDAY: usize = 169;
const SYSCALL_GETPID: usize = 172;
const SYSCALL_GETPPID: usize = 173;
const SYSCALL_GETUID: usize = 174;
const SYSCALL_GETEUID: usize = 175;
const SYSCALL_GETEGID: usize = 177;
const SYSCALL_GETTID: usize = 178;
const SYSCALL_SOCKET: usize = 198;
const SYSCALL_BIND: usize = 200;
const SYSCALL_LISTEN: usize = 201;
const SYSCALL_ACCEPT: usize = 202;
const SYSCALL_CONNECT: usize = 203;
const SYSCALL_GETSOCKNAME: usize = 204;
const SYSCALL_SENDTO: usize = 206;
const SYSCALL_RECVFROM: usize = 207;
const SYSCALL_SETSOCKOPT: usize = 208;
const SYSCALL_BRK: usize = 214;
const SYSCALL_MUNMAP: usize = 215;
const SYSCALL_CLONE: usize = 220;
const SYSCALL_EXECVE: usize = 221;
const SYSCALL_MMAP: usize = 222;
const SYSCALL_MPROTECT: usize = 226;
const SYSCALL_WAIT4: usize = 260;
const SYSCALL_PRLIMIT64: usize = 261;
const SYSCALL_GETRANDOM: usize = 278;
const SYSCALL_MEMBARRIER: usize = 283;

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
        let result: SysRet = match self.cx.a7() {
            SYSCALL_GETCWD => self.sys_getcwd().await,
            SYSCALL_DUP => self.sys_dup(),
            SYSCALL_DUP3 => self.sys_dup3(),
            SYSCALL_FCNTL => self.sys_fcntl(),
            SYSCALL_IOCTL => self.sys_ioctl(),
            SYSCALL_MKDIRAT => self.sys_mkdirat().await,
            SYSCALL_UNLINKAT => self.sys_unlinkat().await,
            SYSCALL_UMOUNT2 => self.sys_umount2().await,
            SYSCALL_MOUNT => self.sys_mount().await,
            SYSCALL_STATFS => self.sys_statfs().await,
            SYSCALL_CHDIR => self.sys_chdir().await,
            SYSCALL_OPENAT => self.sys_openat().await,
            SYSCALL_CLOSE => self.sys_close(),
            SYSCALL_PIPE2 => self.sys_pipe2().await,
            SYSCALL_GETDENTS64 => self.sys_getdents64().await,
            SYSCALL_LSEEK => self.sys_lseek(),
            SYSCALL_READ => self.sys_read().await,
            SYSCALL_WRITE => self.sys_write().await,
            SYSCALL_READV => self.sys_readv().await,
            SYSCALL_WRITEV => self.sys_writev().await,
            SYSCALL_PREAD64 => self.sys_pread64().await,
            SYSCALL_PPOLL => self.sys_ppoll().await,
            SYSCALL_READLINKAT => self.sys_readlinkat().await,
            SYSCALL_NEWFSTATAT => self.sys_newfstatat().await,
            SYSCALL_FSTAT => self.sys_fstat().await,
            SYSCALL_UTIMENSAT => self.sys_utimensat().await,
            SYSCALL_EXIT => self.sys_exit(),
            SYSCALL_EXIT_GROUP => self.sys_exit_group(),
            SYSCALL_SET_TID_ADDRESS => self.sys_set_tid_address(),
            SYSCALL_FUTEX => self.sys_futex().await,
            SYSCALL_SET_ROBUST_LIST => self.sys_set_robust_list().await,
            SYSCALL_GET_ROBUST_LIST => self.sys_get_robust_list().await,
            SYSCALL_NANOSLEEP => self.sys_nanosleep().await,
            SYSCALL_CLOCK_GETTIME => self.sys_clock_gettime().await,
            SYSCALL_SCHED_YIELD => self.sys_sched_yield().await,
            SYSCALL_KILL => self.sys_kill(),
            SYSCALL_TKILL => self.sys_tkill(),
            SYSCALL_TGKILL => self.sys_tgkill(),
            SYSCALL_SIGALTSTACK => self.sys_sigaltstack().await,
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
            SYSCALL_GETRUSAGE => self.sys_getrusage().await,
            SYSCALL_GETTIMEOFDAY => self.sys_gettimeofday().await,
            SYSCALL_GETPID => self.sys_getpid(),
            SYSCALL_GETPPID => self.sys_getppid(),
            SYSCALL_GETUID => self.sys_getuid(),
            SYSCALL_GETEUID => self.sys_geteuid(),
            SYSCALL_GETEGID => self.sys_getegid(),
            SYSCALL_GETTID => self.sys_gettid(),
            SYSCALL_SOCKET => self.sys_socket(),
            SYSCALL_BIND => self.sys_bind(),
            SYSCALL_LISTEN => self.sys_listen(),
            SYSCALL_ACCEPT => self.sys_accept(),
            SYSCALL_CONNECT => self.sys_connect(),
            SYSCALL_GETSOCKNAME => self.sys_getsockname(),
            SYSCALL_SENDTO => self.sys_sendto().await,
            SYSCALL_RECVFROM => self.sys_recvfrom().await,
            SYSCALL_SETSOCKOPT => self.sys_setsockopt(),
            SYSCALL_BRK => self.sys_brk(),
            SYSCALL_MUNMAP => self.sys_munmap(),
            SYSCALL_CLONE => self.sys_clone().await,
            SYSCALL_EXECVE => self.sys_execve().await,
            SYSCALL_MMAP => self.sys_mmap(),
            SYSCALL_MPROTECT => self.sys_mprotect(),
            SYSCALL_WAIT4 => self.sys_wait4().await,
            SYSCALL_PRLIMIT64 => self.sys_prlimit64().await,
            SYSCALL_GETRANDOM => self.sys_getrandom().await,
            SYSCALL_MEMBARRIER => self.sys_membarrier(),
            unknown => panic!("[kernel]unsupported syscall_id: {}", unknown),
        };
        let a0 = match result {
            Ok(a) => a,
            Err(e) => -(e as isize) as usize,
            // Err(_e) => -1isize as usize,
        };
        memory_trace!("syscall return");
        if PRINT_SYSCALL_ALL {
            // println!("syscall return with {}", a0);
            if ![63, 64].contains(&self.cx.a7()) {
                print!("{}", to_yellow!());
                print!("{:?} syscall {} -> ", self.thread.tid(), self.cx.a7(),);
                match result {
                    Ok(n) => print!("{:#x} ", n),
                    Err(e) => print!("{:?} ", e),
                }
                print!("sepc:{:#x}", self.cx.user_sepc);
                println!("{}", reset_color!());
            }
        }
        self.cx.set_user_a0(a0);
        self.do_exit
    }
    /// 线程自己的进程一定不会是退出的状态, 因为进程只有最后一个线程退出后才会析构
    #[inline(always)]
    pub fn alive_then<T>(&mut self, f: impl FnOnce(&mut AliveProcess) -> T) -> T {
        self.process.alive_then(f).unwrap()
    }
    /// 线程自己的进程一定不会是退出的状态, 因为进程只有最后一个线程退出后才会析构
    #[inline(always)]
    pub fn alive_lock(&mut self) -> impl DerefMut<Target = AliveProcess> + '_ {
        AliveGurad(self.process.alive.lock())
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
