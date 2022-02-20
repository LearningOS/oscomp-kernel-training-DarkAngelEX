use crate::trap::context::UKContext;

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

pub struct Syscall<'a> {
    cx: &'a UKContext,
}

impl<'a> Syscall<'a> {
    #[inline(always)]
    pub async fn syscall(&self) -> isize {
        macro_rules! call {
            () => {
                todo!()
            };
            ($sys_fn: expr) => {
                $sys_fn()
            };
        }
        stack_trace!();
        memory_trace!("syscall entry");
        let ret = match self.cx.a7() {
            SYSCALL_DUP => call!(),
            SYSCALL_OPEN => call!(),
            SYSCALL_CLOSE => call!(),
            SYSCALL_PIPE => call!(),
            SYSCALL_READ => call!(),
            SYSCALL_WRITE => call!(),
            SYSCALL_EXIT => call!(),
            SYSCALL_SLEEP => call!(),
            SYSCALL_YIELD => call!(),
            SYSCALL_KILL => call!(),
            SYSCALL_GET_TIME => call!(),
            SYSCALL_GETPID => call!(),
            SYSCALL_FORK => call!(),
            SYSCALL_EXEC => call!(),
            SYSCALL_WAITPID => call!(),
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
            unknown => panic!("[kernel]unsupported syscall_id: {}", unknown),
        };
        memory_trace!("syscall return");
        ret
    }
}
