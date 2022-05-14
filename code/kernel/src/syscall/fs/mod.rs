use alloc::{string::String, sync::Arc};

use crate::{
    fs::{self, pipe, File, Mode, OpenFlags},
    memory::user_ptr::{UserReadPtr, UserWritePtr},
    process::fd::Fd,
    syscall::SysError,
    tools::allocator::from_usize_allocator::FromUsize,
    user::check::UserCheck,
    xdebug::{NO_SYSCALL_PANIC, PRINT_SYSCALL, PRINT_SYSCALL_ALL},
};
pub mod mount;
pub mod stat;

use super::{SysResult, Syscall};

const PRINT_SYSCALL_FS: bool = false || false && PRINT_SYSCALL || PRINT_SYSCALL_ALL;

const AT_FDCWD: isize = -100;

impl Syscall<'_> {
    pub async fn sys_getcwd(&mut self) -> SysResult {
        stack_trace!();
        let (buf_in, len): (UserWritePtr<u8>, usize) = self.cx.into();
        if buf_in.is_null() {
            return Err(SysError::EINVAL);
        }
        let buf = UserCheck::new(self.process)
            .translated_user_writable_slice(buf_in, len)
            .await?;
        let lock = self.alive_lock()?;
        let cwd_len = lock.cwd.path_iter().fold(0, |a, b| a + b.len() + 1) + 1;
        if buf.len() <= cwd_len {
            return Err(SysError::ERANGE);
        }
        let buf = &mut *buf.access_mut();
        let mut buf = &mut buf[..cwd_len];
        for s in lock.cwd.path_iter() {
            buf[0] = b'/';
            buf = buf.split_at_mut(1).1;
            buf[..s.len()].copy_from_slice(s.as_bytes());
            buf = buf.split_at_mut(s.len()).1;
        }
        debug_assert_eq!(buf.len(), 1);
        buf[0] = b'\0';
        return Ok(buf_in.as_usize());
    }
    pub fn sys_dup(&mut self) -> SysResult {
        stack_trace!();
        let fd: usize = self.cx.para1();
        let fd = Fd::from_usize(fd);
        let new = self
            .alive_then(move |a| a.fd_table.dup(fd))?
            .ok_or(SysError::EBADF)?;
        Ok(new.to_usize())
    }
    pub fn sys_dup3(&mut self) -> SysResult {
        stack_trace!();
        let (old_fd, new_fd, flags): (Fd, Fd, u32) = self.cx.into();
        if PRINT_SYSCALL_FS {
            println!("sys_dup3 old{:?} new{:?} flags{:#x}", old_fd, new_fd, flags);
        }
        let flags = OpenFlags::from_bits(flags).unwrap();
        let flags_set = OpenFlags::CLOEXEC;
        if !(flags & !flags_set).is_empty() {
            panic!();
            // return Err(SysError::EINVAL);
        }
        new_fd.in_range()?;
        let close_on_exec = flags.contains(OpenFlags::CLOEXEC);
        self.alive_then(move |a| a.fd_table.replace_dup(old_fd, new_fd, close_on_exec))??;
        Ok(new_fd.to_usize())
    }
    pub async fn sys_getdents64(&mut self) -> SysResult {
        struct Ddirent {
            d_ino: u64,    /* Inode number */
            d_off: u64,    /* Offset to next linux_dirent */
            d_reclen: u16, /* Length of this linux_dirent */
            d_type: u8,
            d_name: (),
            // file_name
            /* Filename (null-terminated) */
            /* length is actually (d_reclen - 2 - offsetof(struct linux_dirent, d_name)) */
            /*
            char           pad;       // Zero padding byte
            char           d_type;    // File type (only since Linux
                                      // 2.6.4); offset is (d_reclen - 1)
            */
        }
        if PRINT_SYSCALL_FS {}
        println!("sys_getdents64 unimplement!");
        if NO_SYSCALL_PANIC {
            todo!()
        }
        return Err(SysError::ENOSYS);
    }
    pub async fn sys_read(&mut self) -> SysResult {
        stack_trace!();
        if PRINT_SYSCALL_FS {
            println!("sys_read");
        }
        let (fd, write_only_buffer) = {
            let (fd, buf, len): (usize, UserWritePtr<u8>, usize) = self.cx.para3();
            let write_only_buffer = UserCheck::new(self.process)
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
            let read_only_buffer = UserCheck::new(self.process)
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
    pub async fn sys_mkdirat(&mut self) -> SysResult {
        stack_trace!();
        let (fd, path, mode): (isize, UserReadPtr<u8>, Mode) = self.cx.into();
        if PRINT_SYSCALL_FS {
            println!("sys_mkdirat {} {:#x} {:#x}", fd, path.as_usize(), mode.0);
        }
        let path = UserCheck::new(self.process)
            .translated_user_array_zero_end(path)
            .await?
            .to_vec();
        let path = String::from_utf8(path)?;
        // 当path为绝对路径时fd被忽略
        // 当path为相对路径时, 如果fd为AT_FDCWD则使用进程工作目录, 否则使用fd
        let flags = fs::OpenFlags::RDWR | fs::OpenFlags::CREAT | fs::OpenFlags::DIRECTORY;

        let base: Arc<dyn File> = match fd {
            AT_FDCWD => self.alive_then(|a| a.cwd.clone())?,
            fd => self
                .alive_then(|a| a.fd_table.get(Fd(fd as usize)).cloned())?
                .ok_or(SysError::EBADF)?,
        };
        let base = base.to_vfs_inode().ok_or(SysError::ENOTDIR)?;
        fs::create_any(base.path_iter(), path.as_str(), flags, mode).await?;
        Ok(0)
    }
    pub async fn sys_unlinkat(&mut self) -> SysResult {
        stack_trace!();
        let (fd, path, flags): (isize, UserReadPtr<u8>, u32) = self.cx.into();
        if flags != 0 {
            panic!("sys_unlinkat flags: {:#x}", flags);
        }
        let path = UserCheck::new(self.process)
            .translated_user_array_zero_end(path)
            .await?
            .to_vec();
        let path = String::from_utf8(path)?;
        let base: Arc<dyn File> = match fd {
            AT_FDCWD => self.alive_then(|a| a.cwd.clone())?,
            fd => self
                .alive_then(|a| a.fd_table.get(Fd(fd as usize)).cloned())?
                .ok_or(SysError::EBADF)?,
        };
        let base = base.to_vfs_inode().ok_or(SysError::ENOTDIR)?;
        fs::unlink(base.path_iter(), &path, OpenFlags::empty()).await?;
        Ok(0)
    }
    pub async fn sys_chdir(&mut self) -> SysResult {
        stack_trace!();
        let path: UserReadPtr<u8> = self.cx.para1();
        let path = UserCheck::new(self.process)
            .translated_user_array_zero_end(path)
            .await?
            .to_vec();
        let path = String::from_utf8(path)?;
        let flags = OpenFlags::RDONLY | fs::OpenFlags::DIRECTORY;
        let inode = fs::open_file(
            self.alive_then(|a| a.cwd.clone())?.path_iter(),
            path.as_str(),
            flags,
            Mode(0o600),
        )
        .await?;
        self.alive_then(|a| a.cwd = inode)?;
        Ok(0)
    }
    pub async fn sys_openat(&mut self) -> SysResult {
        stack_trace!();
        let (fd, path, flags, mode): (isize, UserReadPtr<u8>, u32, Mode) = self.cx.into();
        if PRINT_SYSCALL_FS {
            println!(
                "sys_openat fd: {} path: {:#x} flags: {:#x} mode: {:#o}",
                fd,
                path.as_usize(),
                flags,
                mode.0
            );
        }
        let path = UserCheck::new(self.process)
            .translated_user_array_zero_end(path)
            .await?
            .to_vec();
        let path = String::from_utf8(path)?;
        let flags = fs::OpenFlags::from_bits(flags).unwrap();
        let base: Arc<dyn File> = match fd {
            AT_FDCWD => self.alive_then(|a| a.cwd.clone())?,
            fd => self
                .alive_then(|a| a.fd_table.get(Fd(fd as usize)).cloned())?
                .ok_or(SysError::EBADF)?,
        };
        let base = base.to_vfs_inode().ok_or(SysError::ENOTDIR)?;
        let inode = fs::open_file(base.path_iter(), path.as_str(), flags, mode).await?;
        let close_on_exec = flags.contains(fs::OpenFlags::CLOEXEC);
        let fd = self.alive_lock()?.fd_table.insert(inode, close_on_exec);
        Ok(fd.to_usize())
    }
    pub fn sys_close(&mut self) -> SysResult {
        stack_trace!();
        let fd = self.cx.para1();
        if PRINT_SYSCALL_FS {
            println!("sys_close fd: {}", fd);
        }
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
        let pipe: UserWritePtr<u32> = self.cx.para1();
        let write_to = UserCheck::new(self.process)
            .translated_user_writable_slice(pipe, 2)
            .await?;
        let (reader, writer) = pipe::make_pipe()?;
        let (rfd, wfd) = self.alive_then(move |a| {
            let rfd = a.fd_table.insert(reader, false).to_usize();
            let wfd = a.fd_table.insert(writer, false).to_usize();
            (rfd as u32, wfd as u32)
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
