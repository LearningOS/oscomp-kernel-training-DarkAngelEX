use crate::{
    memory::user_ptr::{UserReadPtr, UserWritePtr},
    process::fd::Fd,
    syscall::{fs::PRINT_SYSCALL_FS, SysError, SysResult, Syscall},
    user::check::UserCheck,
    xdebug::NO_SYSCALL_PANIC,
};

impl Syscall<'_> {
    pub async fn sys_mount(&mut self) -> SysResult {
        stack_trace!();
        let (src, dst, mount_type, flags, data): (
            UserReadPtr<u8>,
            UserReadPtr<u8>,
            UserReadPtr<u8>,
            u32,
            UserReadPtr<u8>,
        ) = self.cx.into();
        if PRINT_SYSCALL_FS {
            println!("sys_mount");
        }
        println!("sys_mount unimplement");
        if NO_SYSCALL_PANIC {
            todo!();
        } else {
            Err(SysError::ENOSYS)
        }
    }
}
