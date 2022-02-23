use core::{fmt, pin::Pin};

use alloc::sync::Arc;

use crate::{
    memory::SpaceGuard,
    process::{thread::Thread, Process},
    trap::context::UKContext,
    xdebug::{stack_trace::StackTrace, PRINT_SYSCALL_ALL},
};

mod fs;
mod process;
mod time;

const SYSCALL_DUP: usize = 24;
const SYSCALL_OPEN: usize = 56;
const SYSCALL_CLOSE: usize = 57;
const SYSCALL_PIPE: usize = 59;
const SYSCALL_READ: usize = 63;
const SYSCALL_WRITE: usize = 64;
const SYSCALL_EXIT: usize = 93;
const SYSCALL_SLEEP: usize = 101;
const SYSCALL_YIELD: usize = 124;
const SYSCALL_KILL: usize = 129;
const SYSCALL_GET_TIME: usize = 169;
const SYSCALL_GETPID: usize = 172;
const SYSCALL_FORK: usize = 220;
const SYSCALL_EXEC: usize = 221;
const SYSCALL_WAITPID: usize = 260;
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
    thread_arc: &'a Arc<Thread>,
    process: &'a Process,
    do_exit: bool,
    stack_trace: Pin<&'a mut StackTrace>,
}

impl<'a> Syscall<'a> {
    pub fn new(
        cx: &'a mut UKContext,
        thread_arc: &'a Arc<Thread>,
        process: &'a Process,
        stack_trace: Pin<&'a mut StackTrace>,
    ) -> Self {
        Self {
            cx,
            thread: thread_arc.as_ref(),
            thread_arc,
            process,
            do_exit: false,
            stack_trace,
        }
    }
    /// return do_exit
    #[inline(always)]
    pub async fn syscall(&mut self) -> bool {
        stack_trace!(self.stack_trace.ptr_usize());
        memory_trace!("syscall entry");
        self.cx.into_next_instruction();
        let result: SysResult = match self.cx.a7() {
            SYSCALL_DUP => todo!(),
            SYSCALL_OPEN => todo!(),
            SYSCALL_CLOSE => todo!(),
            SYSCALL_PIPE => todo!(),
            SYSCALL_READ => self.sys_read().await,
            SYSCALL_WRITE => self.sys_write().await,
            SYSCALL_EXIT => self.sys_exit(),
            SYSCALL_SLEEP => self.sys_sleep().await,
            SYSCALL_YIELD => self.sys_yield().await,
            SYSCALL_KILL => todo!(),
            SYSCALL_GET_TIME => self.sys_gettime(),
            SYSCALL_GETPID => self.sys_getpid(),
            SYSCALL_FORK => self.sys_fork(),
            SYSCALL_EXEC => self.sys_exec(),
            SYSCALL_WAITPID => self.sys_waitpid().await,
            SYSCALL_THREAD_CREATE => todo!(),
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
    // if return Err will set do_exit = true
    #[inline(always)]
    pub fn using_space_then<T>(
        &mut self,
        f: impl FnOnce(SpaceGuard) -> T,
    ) -> Result<T, UniqueSysError<{ SysError::ESRCH as isize }>> {
        self.process.using_space_then(f).map_err(|()| {
            self.do_exit = true;
            UniqueSysError
        })
    }
}

pub type SysResult = Result<usize, SysError>;

#[allow(dead_code)]
#[repr(isize)]
#[derive(Debug)]
pub enum SysError {
    EUNDEF = 0,
    EPERM = 1,
    ENOENT = 2,
    ESRCH = 3,
    EINTR = 4,
    EIO = 5,
    ENXIO = 6,
    E2BIG = 7,
    ENOEXEC = 8,
    EBADF = 9,
    ECHILD = 10,
    EAGAIN = 11,
    ENOMEM = 12,
    EACCES = 13,
    EFAULT = 14,
    ENOTBLK = 15,
    EBUSY = 16,
    EEXIST = 17,
    EXDEV = 18,
    ENODEV = 19,
    ENOTDIR = 20,
    EISDIR = 21,
    EINVAL = 22,
    ENFILE = 23,
    EMFILE = 24,
    ENOTTY = 25,
    ETXTBSY = 26,
    EFBIG = 27,
    ENOSPC = 28,
    ESPIPE = 29,
    EROFS = 30,
    EMLINK = 31,
    EPIPE = 32,
    EDOM = 33,
    ERANGE = 34,
    EDEADLK = 35,
    ENAMETOOLONG = 36,
    ENOLCK = 37,
    ENOSYS = 38,
    ENOTEMPTY = 39,
    ELOOP = 40,
    EIDRM = 43,
    ENOTSOCK = 80,
    ENOPROTOOPT = 92,
    EPFNOSUPPORT = 96,
    EAFNOSUPPORT = 97,
    ENOBUFS = 105,
    EISCONN = 106,
    ENOTCONN = 107,
    ETIMEDOUT = 110,
    ECONNREFUSED = 111,
}

#[allow(non_snake_case)]
impl fmt::Display for SysError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::SysError::*;
        write!(
            f,
            "{}",
            match self {
                EPERM => "Operation not permitted",
                ENOENT => "No such file or directory",
                ESRCH => "No such process",
                EINTR => "Interrupted system call",
                EIO => "I/O error",
                ENXIO => "No such device or address",
                E2BIG => "Argument list too long",
                ENOEXEC => "Exec format error",
                EBADF => "Bad file number",
                ECHILD => "No child processes",
                EAGAIN => "Try again",
                ENOMEM => "Out of memory",
                EACCES => "Permission denied",
                EFAULT => "Bad address",
                ENOTBLK => "Block device required",
                EBUSY => "Device or resource busy",
                EEXIST => "File exists",
                EXDEV => "Cross-device link",
                ENODEV => "No such device",
                ENOTDIR => "Not a directory",
                EISDIR => "Is a directory",
                EINVAL => "Invalid argument",
                ENFILE => "File table overflow",
                EMFILE => "Too many open files",
                ENOTTY => "Not a typewriter",
                ETXTBSY => "Text file busy",
                EFBIG => "File too large",
                ENOSPC => "No space left on device",
                ESPIPE => "Illegal seek",
                EROFS => "Read-only file system",
                EMLINK => "Too many links",
                EPIPE => "Broken pipe",
                EDOM => "Math argument out of domain of func",
                ERANGE => "Math result not representable",
                EDEADLK => "Resource deadlock would occur",
                ENAMETOOLONG => "File name too long",
                ENOLCK => "No record locks available",
                ENOSYS => "Function not implemented",
                ENOTEMPTY => "Directory not empty",
                ELOOP => "Too many symbolic links encountered",
                ENOTSOCK => "Socket operation on non-socket",
                ENOPROTOOPT => "Protocol not available",
                EPFNOSUPPORT => "Protocol family not supported",
                EAFNOSUPPORT => "Address family not supported by protocol",
                ENOBUFS => "No buffer space available",
                EISCONN => "Transport endpoint is already connected",
                ENOTCONN => "Transport endpoint is not connected",
                ECONNREFUSED => "Connection refused",
                _ => "Unknown error",
            },
        )
    }
}

// zero-size SysError
pub struct UniqueSysError<const X: isize>;

impl<const X: isize> From<UniqueSysError<X>> for SysError {
    fn from(_: UniqueSysError<X>) -> Self {
        unsafe { core::mem::transmute(X) }
    }
}
