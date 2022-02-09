const SYSCALL_READ: usize = 63;
const SYSCALL_WRITE: usize = 64;
const SYSCALL_EXIT: usize = 93;
const SYSCALL_YIELD: usize = 124;
const SYSCALL_GET_TIME: usize = 169;
const SYSCALL_GETPID: usize = 172;
const SYSCALL_FORK: usize = 220;
const SYSCALL_EXEC: usize = 221;
const SYSCALL_WAITPID: usize = 260;

#[inline(always)]
pub fn syscall(syscall_id: usize, _args: [usize; 3]) -> isize {
    match syscall_id {
        SYSCALL_READ => todo!(),
        SYSCALL_WRITE => todo!(),
        SYSCALL_EXIT => todo!(),
        SYSCALL_YIELD => todo!(),
        SYSCALL_GET_TIME => todo!(),
        SYSCALL_GETPID => todo!(),
        SYSCALL_FORK => todo!(),
        SYSCALL_EXEC => todo!(),
        SYSCALL_WAITPID => todo!(),
        _ => panic!("Unsupported syscall_id: {}", syscall_id),
    }
}
