use crate::trap::context::TrapContext;

mod fs;
mod process;

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
    match syscall_id {
        SYSCALL_DUP => todo!(),
        SYSCALL_OPEN => todo!(),
        SYSCALL_CLOSE => todo!(),
        SYSCALL_PIPE => todo!(),
        SYSCALL_READ => fs::sys_read(trap_context, send_parameter(args)),
        SYSCALL_WRITE => fs::sys_write(trap_context, send_parameter(args)),
        SYSCALL_EXIT => todo!(),
        SYSCALL_SLEEP => todo!(),
        SYSCALL_YIELD => todo!(),
        SYSCALL_KILL => todo!(),
        SYSCALL_GET_TIME => todo!(),
        SYSCALL_GETPID => todo!(),
        SYSCALL_FORK => process::sys_fork(trap_context, send_parameter(args)),
        SYSCALL_EXEC => process::sys_exec(trap_context, send_parameter(args)),
        SYSCALL_WAITPID => todo!(),
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
        _ => panic!("[kernel]unsupported syscall_id: {}", syscall_id),
    }
}

pub fn assert_fork(a0: usize) -> isize {
    match a0 {
        SYSCALL_FORK => 0,
        _ => panic!(),
    }
}
