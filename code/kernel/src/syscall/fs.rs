use alloc::string::String;

use crate::{
    fs::{self, pipe},
    process::fd::Fd,
    syscall::SysError,
    tools::allocator::from_usize_allocator::FromUsize,
    xdebug::{PRINT_SYSCALL, PRINT_SYSCALL_ALL},
};

use super::{SysResult, Syscall};

const PRINT_SYSCALL_FS: bool = false || false && PRINT_SYSCALL || PRINT_SYSCALL_ALL;

impl<'a> Syscall<'a> {
    pub fn sys_dup(&mut self) -> SysResult {
        let fd: usize = self.cx.parameter1();
        let fd = Fd::from_usize(fd);
        let new = self
            .alive_then(move |a| a.fd_table.dup(fd))?
            .ok_or_else(|| SysError::ENFILE)?;
        Ok(new.into_usize())
    }
    pub async fn sys_read(&mut self) -> SysResult {
        if PRINT_SYSCALL_FS {
            println!("sys_read");
        }
        let (fd, write_only_buffer) = {
            let (fd, buf, len): (usize, *mut u8, usize) = self.cx.parameter3();
            let write_only_buffer = self
                .using_space()?
                .translated_user_writable_slice(buf, len)?;
            (fd, write_only_buffer)
        };
        let file = self
            .alive_then(move |a| a.fd_table.get(Fd::new(fd)).map(|p| p.clone()))?
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
            let read_only_buffer = self
                .using_space()?
                .translated_user_readonly_slice(buf, len)?;
            (fd, read_only_buffer)
        };
        let file = self
            .alive_then(move |a| a.fd_table.get(Fd::new(fd)).map(|p| p.clone()))?
            .ok_or(SysError::EBADF)?;
        if !file.writable() {
            return Err(SysError::EPERM);
        }
        file.write(self.process_arc.clone(), read_only_buffer).await
    }
    pub async fn sys_open(&mut self) -> SysResult {
        if PRINT_SYSCALL_FS {
            println!("sys_open");
        }
        let (path, flags) = {
            let (path, flags): (*const u8, u32) = self.cx.parameter2();
            let space_guard = self.using_space()?;
            let path = space_guard
                .translated_user_array_zero_end(path)?
                .into_vec(&space_guard);
            drop(space_guard);
            (String::from_utf8(path)?, flags)
        };
        let inode = fs::open_file(path.as_str(), fs::OpenFlags::from_bits(flags).unwrap())
            .ok_or(SysError::ENFILE)?;
        let fd = self.alive_then(move |a| a.fd_table.insert(inode))?;
        Ok(fd.into_usize())
    }
    pub fn sys_close(&mut self) -> SysResult {
        if PRINT_SYSCALL_FS {
            println!("sys_close");
        }
        let fd = self.cx.parameter1();
        let fd = Fd::new(fd);
        let file = self
            .alive_then(move |a| a.fd_table.remove(fd))?
            .ok_or(SysError::EBADF)?;
        drop(file); // just for clarity
        Ok(0)
    }
    pub fn sys_pipe(&mut self) -> SysResult {
        let pipe: *mut usize = self.cx.parameter1();
        let space_guard = self.using_space()?;
        let write_to = space_guard.translated_user_writable_slice(pipe, 2)?;
        let (reader, writer) = pipe::make_pipe()?;
        let (rfd, wfd) = self.alive_then(move |a| {
            let rfd = a.fd_table.insert(reader).into_usize();
            let wfd = a.fd_table.insert(writer).into_usize();
            (rfd, wfd)
        })?;
        write_to
            .access_mut(&space_guard)
            .copy_from_slice(&[rfd, wfd]);
        Ok(0)
    }
}
