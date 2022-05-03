use crate::{
    fs::stat::Stat,
    memory::user_ptr::UserWritePtr,
    process::fd::Fd,
    syscall::{fs::PRINT_SYSCALL_FS, SysError, SysResult, Syscall},
    user::check::UserCheck,
};

impl Syscall<'_> {
    pub async fn sys_fstat(&mut self) -> SysResult {
        stack_trace!();
        let (fd, path): (isize, UserWritePtr<Stat>) = self.cx.into();
        if PRINT_SYSCALL_FS {
            println!("sys_fstat fd {:?} path {:?}", fd, path.as_usize());
        }
        let buf = UserCheck::new(self.process)
            .translated_user_writable_value(path)
            .await?;
        if fd < 0 {
            return Err(SysError::EINVAL);
        }
        let fd = self
            .alive_then(|a| a.fd_table.get(Fd(fd as usize)).cloned())?
            .ok_or(SysError::EBADF)?;
        let mut stat = Stat::zeroed();
        fd.stat(&mut stat).await?;
        buf.store(stat);
        Ok(0)
    }
}
