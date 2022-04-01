use alloc::string::String;

use crate::{
    fs::{self, pipe},
    memory::user_ptr::{UserReadPtr, UserWritePtr},
    process::fd::Fd,
    syscall::SysError,
    tools::allocator::from_usize_allocator::FromUsize,
    user::check::UserCheck,
    xdebug::{PRINT_SYSCALL, PRINT_SYSCALL_ALL},
};

use super::{SysResult, Syscall};

const PRINT_SYSCALL_FS: bool = false || false && PRINT_SYSCALL || PRINT_SYSCALL_ALL;

impl<'a> Syscall<'a> {
    pub fn sys_dup(&mut self) -> SysResult {
        stack_trace!();
        let fd: usize = self.cx.para1();
        let fd = Fd::from_usize(fd);
        let new = self
            .alive_then(move |a| a.fd_table.dup(fd))?
            .ok_or(SysError::ENFILE)?;
        Ok(new.to_usize())
    }
    pub async fn sys_read(&mut self) -> SysResult {
        stack_trace!();
        if PRINT_SYSCALL_FS {
            println!("sys_read");
        }
        let (fd, write_only_buffer) = {
            let (fd, buf, len): (usize, UserWritePtr<u8>, usize) = self.cx.para3();
            let write_only_buffer = UserCheck::new()
                .translated_user_writable_slice(buf, len)
                .await?;
            (fd, write_only_buffer)
        };
        let file = self
            .alive_then(move |a| a.fd_table.get(Fd::new(fd)).cloned())?
            .ok_or(SysError::EBADF)?;
        if !file.readable() {
            return Err(SysError::EPERM);
        }
        file.read(write_only_buffer).await
    }
    pub async fn sys_write(&mut self) -> SysResult {
        stack_trace!();
        if PRINT_SYSCALL_FS {
            println!("sys_write");
        }
        let (fd, read_only_buffer) = {
            let (fd, buf, len): (usize, UserReadPtr<u8>, usize) = self.cx.para3();
            let read_only_buffer = UserCheck::new()
                .translated_user_readonly_slice(buf, len)
                .await?;
            (fd, read_only_buffer)
        };
        let file = self
            .alive_then(move |a| a.fd_table.get(Fd::new(fd)).cloned())?
            .ok_or(SysError::EBADF)?;
        if !file.writable() {
            return Err(SysError::EPERM);
        }
        let ret = file.write(read_only_buffer).await;
        ret
    }
    pub async fn sys_open(&mut self) -> SysResult {
        stack_trace!();
        if PRINT_SYSCALL_FS {
            println!("sys_open");
        }
        let (path, flags) = {
            let (path, flags): (UserReadPtr<u8>, u32) = self.cx.para2();
            let path = UserCheck::new()
                .translated_user_array_zero_end(path)
                .await?
                .to_vec();
            (String::from_utf8(path)?, flags)
        };
        let inode = fs::open_file(path.as_str(), fs::OpenFlags::from_bits(flags).unwrap())
            .ok_or(SysError::ENFILE)?;
        let fd = self.alive_then(move |a| a.fd_table.insert(inode))?;
        Ok(fd.to_usize())
    }
    pub fn sys_close(&mut self) -> SysResult {
        stack_trace!();
        if PRINT_SYSCALL_FS {
            println!("sys_close");
        }
        let fd = self.cx.para1();
        let fd = Fd::new(fd);
        let file = self
            .alive_then(move |a| a.fd_table.remove(fd))?
            .ok_or(SysError::EBADF)?;
        drop(file); // just for clarity
        Ok(0)
    }
    pub async fn sys_pipe(&mut self) -> SysResult {
        stack_trace!();
        // println!("sys_pipe");
        let pipe: UserWritePtr<usize> = self.cx.para1();
        let write_to = UserCheck::new()
            .translated_user_writable_slice(pipe, 2)
            .await?;
        let (reader, writer) = pipe::make_pipe()?;
        let (rfd, wfd) = self.alive_then(move |a| {
            let rfd = a.fd_table.insert(reader).to_usize();
            let wfd = a.fd_table.insert(writer).to_usize();
            (rfd, wfd)
        })?;
        write_to.access_mut().copy_from_slice(&[rfd, wfd]);
        Ok(0)
    }
    pub fn sys_ioctl(&mut self) -> SysResult {
        stack_trace!();
        let (fd, cmd, arg): (usize, u32, usize) = self.cx.into();
        self.alive_then(|a| a.fd_table.get(Fd::new(fd)).cloned())?
            .ok_or(SysError::ENFILE)?
            .ioctl(cmd, arg)
    }
}
