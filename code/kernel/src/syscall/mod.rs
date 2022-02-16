use crate::trap::context::TrapContext;

mod fs;
mod process;
mod sync;

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

#[inline(always)]
pub fn syscall(trap_context: &mut TrapContext, syscall_id: usize, args: [usize; 3]) -> isize {
    #[inline(always)]
    fn send_parameter<const N: usize>(args: [usize; 3]) -> [usize; N] {
        *args.split_array_ref().0
    }
    macro_rules! call {
        () => {
            todo!()
        };
        ($sys_fn: expr) => {
            $sys_fn(trap_context, send_parameter(args))
        };
    }
    stack_trace!();
    memory_trace!("syscall entry");
    let ret = match syscall_id {
        SYSCALL_DUP => call!(),
        SYSCALL_OPEN => call!(),
        SYSCALL_CLOSE => call!(),
        SYSCALL_PIPE => call!(),
        SYSCALL_READ => call!(fs::sys_read),
        SYSCALL_WRITE => call!(fs::sys_write),
        SYSCALL_EXIT => call!(process::sys_exit),
        SYSCALL_SLEEP => call!(sync::sys_sleep),
        SYSCALL_YIELD => call!(process::sys_yield),
        SYSCALL_KILL => call!(process::sys_kill),
        SYSCALL_GET_TIME => call!(process::sys_get_time),
        SYSCALL_GETPID => call!(process::sys_getpid),
        SYSCALL_FORK => call!(process::sys_fork),
        SYSCALL_EXEC => call!(process::sys_exec),
        SYSCALL_WAITPID => call!(process::sys_waitpid),
        SYSCALL_THREAD_CREATE => call!(),
        SYSCALL_GETTID => call!(),
        SYSCALL_WAITTID => call!(),
        SYSCALL_MUTEX_CREATE => call!(),
        SYSCALL_MUTEX_LOCK => call!(),
        SYSCALL_MUTEX_UNLOCK => call!(),
        SYSCALL_SEMAPHORE_CREATE => call!(),
        SYSCALL_SEMAPHORE_UP => call!(),
        SYSCALL_SEMAPHORE_DOWN => call!(),
        SYSCALL_CONDVAR_CREATE => call!(),
        SYSCALL_CONDVAR_SIGNAL => call!(),
        SYSCALL_CONDVAR_WAIT => call!(),
        _ => panic!("[kernel]unsupported syscall_id: {}", syscall_id),
    };
    memory_trace!("syscall return");
    ret
}

pub fn assert_fork(a0: usize) -> isize {
    match a0 {
        SYSCALL_FORK => 0,
        _ => panic!(),
    }
}
