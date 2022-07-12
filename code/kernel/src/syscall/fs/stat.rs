use ftl_util::{
    fs::{stat::Stat, Mode, OpenFlags},
    time::TimeSpec,
};

use crate::{
    memory::user_ptr::{UserReadPtr, UserWritePtr},
    process::fd::Fd,
    syscall::{fs::PRINT_SYSCALL_FS, SysError, SysRet, Syscall},
    user::check::UserCheck,
};

impl Syscall<'_> {
    pub async fn sys_fstat(&mut self) -> SysRet {
        stack_trace!();
        let (fd, statbuf): (isize, UserWritePtr<Stat>) = self.cx.into();
        if PRINT_SYSCALL_FS {
            println!("sys_fstat fd {:?} path {:#x}", fd, statbuf.as_usize());
        }
        let buf = UserCheck::new(self.process).writable_value(statbuf).await?;
        if fd < 0 {
            return Err(SysError::EINVAL);
        }
        let inode = self
            .alive_then(|a| a.fd_table.get(Fd(fd as usize)).cloned())
            .ok_or(SysError::EBADF)?;
        let mut stat = Stat::zeroed();
        inode.stat(&mut stat).await?;
        buf.store(stat);
        Ok(0)
    }
    /// times[0]: access time
    ///
    /// times[1]: modify time
    pub async fn sys_utimensat(&mut self) -> SysRet {
        let (fd, path, times, flags): (isize, UserReadPtr<u8>, UserReadPtr<[TimeSpec; 2]>, u32) =
            self.cx.into();
        if PRINT_SYSCALL_FS {
            println!(
                "sys_utimensat fd {:?} path {:#x} times: {:#x} flags: {:#x}",
                fd,
                path.as_usize(),
                times.as_usize(),
                flags
            );
        }
        let times = if times.is_null() {
            [TimeSpec::NOW, TimeSpec::NOW]
        } else {
            UserCheck::new(self.process)
                .readonly_value(times)
                .await?
                .load()
        };
        if path.is_null() {
            self.alive_then(|a| a.fd_table.get(Fd(fd as usize)).cloned())
                .ok_or(SysError::EBADF)?
        } else {
            self.fd_path_open(fd, path, OpenFlags::RDONLY, Mode(0o600))
                .await?
        }
        .utimensat(times)
        .await
    }
    pub async fn sys_newfstatat(&mut self) -> SysRet {
        stack_trace!();
        let (fd, path, statbuf, flags): (isize, UserReadPtr<u8>, UserWritePtr<Stat>, u32) =
            self.cx.into();
        if PRINT_SYSCALL_FS {
            println!(
                "sys_newfstatat fd {:?} path {:#x} buf: {:#x} flags: {:#x}",
                fd,
                path.as_usize(),
                statbuf.as_usize(),
                flags
            );
        }
        let buf = UserCheck::new(self.process).writable_value(statbuf).await?;
        let inode = self
            .fd_path_open(fd, path, OpenFlags::RDONLY, Mode(0o600))
            .await?;
        let mut stat = Stat::zeroed();
        inode.stat(&mut stat).await?;
        buf.store(stat);
        Ok(0)
    }
}
