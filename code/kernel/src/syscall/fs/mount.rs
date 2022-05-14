use crate::{
    memory::user_ptr::{UserReadPtr, UserWritePtr},
    process::fd::Fd,
    syscall::{fs::PRINT_SYSCALL_FS, SysError, SysResult, Syscall},
    user::check::UserCheck,
    xdebug::NO_SYSCALL_PANIC,
};

impl Syscall<'_> {
    ///
    ///
    ///
    pub async fn sys_mount(&mut self) -> SysResult {
        stack_trace!();
        let (src, dst, mount_type, _flags, _data): (
            UserReadPtr<u8>,
            UserReadPtr<u8>,
            UserReadPtr<u8>,
            u32,
            UserReadPtr<u8>,
        ) = self.cx.into();
        if PRINT_SYSCALL_FS {
            println!("sys_mount");
        }
        let _src = UserCheck::new(self.process)
            .translated_user_array_zero_end(src)
            .await?;
        let _dst = UserCheck::new(self.process)
            .translated_user_array_zero_end(dst)
            .await?;
        let _mount_type = UserCheck::new(self.process)
            .translated_user_array_zero_end(mount_type)
            .await?;
        if false {
            println!("sys_mount unimplement");
            return Err(SysError::ENOSYS);
        }
        Ok(0)
    }
    pub async fn sys_umount2(&mut self) -> SysResult {
        let (target, _flags): (UserReadPtr<u8>, u32) = self.cx.into();
        let _target = UserCheck::new(self.process)
            .translated_user_array_zero_end(target)
            .await?;
        Ok(0)
    }
}
