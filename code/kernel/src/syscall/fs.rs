use crate::{
    process::fd::Fd,
    syscall::SysError,
    xdebug::{PRINT_SYSCALL, PRINT_SYSCALL_ALL},
};

use super::{SysResult, Syscall};

const PRINT_SYSCALL_FS: bool = false && PRINT_SYSCALL || PRINT_SYSCALL_ALL;

impl<'a> Syscall<'a> {
    pub async fn sys_read(&mut self) -> SysResult {
        if PRINT_SYSCALL_FS {
            println!("sys_read");
        }
        let (fd, write_only_buffer) = {
            let (fd, buf, len): (usize, *mut u8, usize) = self.cx.parameter3();
            let guard = self.using_space()?;
            let write_only_buffer = guard.translated_user_writable_slice(buf, len)?;
            (fd, write_only_buffer)
        };
        let file = self
            .process
            .alive_then(|a| a.fd_table.get(Fd::new(fd)).map(|p| p.clone()))
            .map_err(|_e| SysError::ESRCH)?
            .ok_or(SysError::EBADF)?;
        if !file.readable() {
            return Err(SysError::EPERM);
        }
        file.read(self.process_arc.clone(), write_only_buffer).await
    }
    pub async fn sys_write(&mut self) -> SysResult {
        if PRINT_SYSCALL_FS {
            println!("sys_write");
        }
        let (fd, read_only_buffer) = {
            let (fd, buf, len): (usize, *const u8, usize) = self.cx.parameter3();
            let guard = self.using_space()?;
            let read_only_buffer = guard.translated_user_readonly_slice(buf, len)?;
            (fd, read_only_buffer)
        };
        let file = self
            .process
            .alive_then(|a| a.fd_table.get(Fd::new(fd)).map(|p| p.clone()))
            .map_err(|_e| SysError::ESRCH)?
            .ok_or(SysError::EBADF)?;
        if !file.writable() {
            return Err(SysError::EPERM);
        }
        file.write(self.process_arc.clone(), read_only_buffer).await
    }
}
