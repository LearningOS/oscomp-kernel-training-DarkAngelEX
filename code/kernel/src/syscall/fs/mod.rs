use core::{ptr::addr_of_mut, sync::atomic::Ordering};

use alloc::{string::String, sync::Arc, vec::Vec};
use ftl_util::{
    error::SysR,
    fs::{Mode, OpenFlags, Seek},
};
use vfs::{File, VfsFile};

use crate::{
    fs::{self, pipe, Iovec},
    memory::user_ptr::{UserInOutPtr, UserReadPtr, UserWritePtr},
    process::fd::Fd,
    syscall::SysError,
    tools::{allocator::from_usize_allocator::FromUsize, path},
    user::check::UserCheck,
    xdebug::{PRINT_SYSCALL, PRINT_SYSCALL_ALL, PRINT_SYSCALL_RW},
};
pub mod mount;
mod select;
pub mod stat;

use super::{SysRet, Syscall};

const PRINT_SYSCALL_FS: bool = false || false && PRINT_SYSCALL || PRINT_SYSCALL_ALL;

const AT_FDCWD: isize = -100;
const AT_SYMLINK_NOFOLLOW: usize = 1 << 8;
const AT_EACCESS: usize = 1 << 9;
const AT_REMOVEDIR: usize = 1 << 9;

impl Syscall<'_> {
    pub async fn fd_path_impl(
        &mut self,
        fd: isize,
        path: UserReadPtr<u8>,
    ) -> SysR<(SysR<Arc<VfsFile>>, String)> {
        let path = UserCheck::new(self.process)
            .array_zero_end(path)
            .await?
            .to_vec();
        let path = String::from_utf8(path)?;
        let file: SysR<Arc<dyn File>> = if !path::is_absolute_path(&path) {
            match fd {
                AT_FDCWD => Ok(self.alive_then(|a| a.cwd.clone())),
                fd => self
                    .alive_then(|a| a.fd_table.get(Fd(fd as usize)).cloned())
                    .ok_or(SysError::EBADF),
            }
        } else {
            Err(SysError::EBADF)
        };
        Ok((file.and_then(|v| v.into_vfs_file()), path))
    }
    pub async fn fd_path_open(
        &mut self,
        fd: isize,
        path: UserReadPtr<u8>,
        flags: OpenFlags,
        mode: Mode,
    ) -> SysR<Arc<VfsFile>> {
        let (base, path) = self.fd_path_impl(fd, path).await?;
        if PRINT_SYSCALL_FS {
            println!("fd_path_open path: {}", path);
        }
        fs::open_file((base, path.as_str()), flags, mode).await
    }
    pub async fn fd_path_create_any(
        &mut self,
        fd: isize,
        path: UserReadPtr<u8>,
        flags: OpenFlags,
        mode: Mode,
    ) -> SysR<Arc<VfsFile>> {
        let (base, path) = self.fd_path_impl(fd, path).await?;
        fs::create_any((base, path.as_str()), flags, mode).await
    }
    pub async fn sys_getcwd(&mut self) -> SysRet {
        stack_trace!();
        let (buf_in, len): (UserWritePtr<u8>, usize) = self.cx.into();
        if PRINT_SYSCALL_FS {
            println!("sys_getcwd");
        }
        if buf_in.is_null() {
            return Err(SysError::EINVAL);
        }
        let buf = UserCheck::new(self.process)
            .writable_slice(buf_in, len)
            .await?;
        let path = self.alive_then(|a| a.cwd.path_str());
        let cwd_len = path.iter().fold(0, |a, b| a + b.len() + 1) + 1;
        let cwd_len = cwd_len.max(2);
        if buf.len() <= cwd_len {
            return Err(SysError::ERANGE);
        }
        let buf = &mut *buf.access_mut();
        let mut buf = &mut buf[..cwd_len];
        if path.is_empty() {
            buf[0] = b'/';
            buf = &mut buf[1..];
        }
        for s in path {
            buf[0] = b'/';
            buf = &mut buf[1..];
            buf[..s.len()].copy_from_slice(s.as_bytes());
            buf = &mut buf[s.len()..];
        }
        debug_assert_eq!(buf.len(), 1);
        buf[0] = b'\0';
        Ok(buf_in.as_usize())
    }
    pub fn sys_dup(&mut self) -> SysRet {
        stack_trace!();
        let fd: usize = self.cx.para1();
        let fd = Fd::from_usize(fd);
        let new = self.alive_then(move |a| a.fd_table.dup(fd))?;
        Ok(new.to_usize())
    }
    pub fn sys_dup3(&mut self) -> SysRet {
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
        self.alive_then(move |a| a.fd_table.replace_dup(old_fd, new_fd, flags))?;
        Ok(new_fd.0)
    }
    pub async fn sys_getdents64(&mut self) -> SysRet {
        stack_trace!();
        #[repr(C)]
        struct Ddirent {
            d_ino: u64,    /* Inode number */
            d_off: u64,    /* Offset to next linux_dirent */
            d_reclen: u16, /* Length of this linux_dirent */
            d_type: u8,
            d_name: (),
        }
        let (fd, dirp, count): (Fd, UserWritePtr<u8>, usize) = self.cx.into();
        if PRINT_SYSCALL_FS {
            println!(
                "sys_getdents64 fd: {:?} dirp: {:#x} count: {}",
                fd,
                dirp.as_usize(),
                count
            );
        }
        let align = core::mem::align_of::<Ddirent>();
        if dirp.as_usize() % align != 0 {
            return Err(SysError::EFAULT);
        }
        let dirp = UserCheck::new(self.process)
            .writable_slice(dirp, count)
            .await?;
        let file = self
            .alive_then(|a| a.fd_table.get(fd).cloned())
            .ok_or(SysError::EBADF)?;
        let file = file.into_vfs_file()?;
        let list = file.list().await?;
        let offset = file.ptr.load(Ordering::Relaxed);
        if offset >= list.len() {
            return Ok(0);
        }
        let mut buffer = &mut *dirp.access_mut();
        let mut cnt = 0;
        for (dt, name) in &list[offset..] {
            let ptr = buffer.as_mut_ptr();
            debug_assert_eq!(ptr as usize % align, 0);
            unsafe {
                let dirent_ptr = ptr.cast::<Ddirent>();
                let name_ptr = addr_of_mut!((*dirent_ptr).d_name).cast::<u8>();
                let end_ptr = name_ptr.add(name.len() + 1);
                let align_add = end_ptr.align_offset(align);
                let this_len = end_ptr.offset_from(ptr) as usize + align_add;
                if this_len > buffer.len() {
                    break;
                }
                let dirent = &mut *dirent_ptr;
                dirent.d_ino = 1;
                dirent.d_off = this_len as u64;
                dirent.d_reclen = this_len as u16;
                dirent.d_type = *dt as u8; // <- no implement
                let name_buf = core::ptr::slice_from_raw_parts_mut(name_ptr, name.len() + 1);
                (&mut *name_buf)[..name.len()].copy_from_slice(name.as_bytes());
                (&mut *name_buf)[name.len()] = b'\0';
                cnt += this_len;
                buffer = &mut buffer[this_len..];
                file.ptr
                    .store(file.ptr.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
            }
        }
        Ok(cnt)
    }
    pub fn sys_lseek(&mut self) -> SysRet {
        let (fd, offset, whence): (Fd, isize, u32) = self.cx.into();
        if PRINT_SYSCALL_FS {
            println!("sys_lseek");
        }
        let file = self
            .alive_then(|p| p.fd_table.get(fd).cloned())
            .ok_or(SysError::EBADF)?;
        let whence = Seek::from_user(whence)?;
        file.lseek(offset, whence)
    }
    pub async fn sys_read(&mut self) -> SysRet {
        stack_trace!();
        let (fd, buf, len): (usize, UserWritePtr<u8>, usize) = self.cx.into();
        if PRINT_SYSCALL_FS && PRINT_SYSCALL_RW {
            println!("sys_read len: {}", len);
        }
        let buf = UserCheck::new(self.process)
            .writable_slice(buf, len)
            .await?;
        let file = self
            .alive_then(move |a| a.fd_table.get(Fd::new(fd)).cloned())
            .ok_or(SysError::EBADF)?;
        if !file.readable() {
            return Err(SysError::EPERM);
        }
        let len = file.read(&mut *buf.access_mut()).await?;
        Ok(len)
    }
    pub async fn sys_write(&mut self) -> SysRet {
        stack_trace!();
        let (fd, buf, len): (usize, UserReadPtr<u8>, usize) = self.cx.into();
        if PRINT_SYSCALL_FS && PRINT_SYSCALL_RW {
            println!("sys_write len: {}", len);
        }
        let buf = UserCheck::new(self.process)
            .readonly_slice(buf, len)
            .await?;
        let file = self
            .alive_then(move |a| a.fd_table.get(Fd::new(fd)).cloned())
            .ok_or(SysError::EBADF)?;
        if !file.writable() {
            return Err(SysError::EPERM);
        }
        let ret = file.write(&*buf.access()).await;
        ret
    }
    pub async fn sys_readv(&mut self) -> SysRet {
        stack_trace!();
        if PRINT_SYSCALL_FS {
            println!("sys_readv");
        }
        let (fd, iov, vlen): (usize, UserReadPtr<Iovec>, usize) = self.cx.into();
        let file = self
            .alive_then(move |a| a.fd_table.get(Fd::new(fd)).cloned())
            .ok_or(SysError::EBADF)?;
        if !file.readable() {
            return Err(SysError::EPERM);
        }
        let uc = UserCheck::new(self.process);
        let vbuf = uc.readonly_slice(iov, vlen).await?;
        let mut cnt = 0;
        for &Iovec { iov_base, iov_len } in vbuf.access().iter() {
            let buf = uc.writable_slice(iov_base, iov_len).await?;
            cnt += file.read(&mut *buf.access_mut()).await?;
        }
        Ok(cnt)
    }
    pub async fn sys_writev(&mut self) -> SysRet {
        stack_trace!();
        if PRINT_SYSCALL_FS {
            println!("sys_writev");
        }
        let (fd, iov, vlen): (usize, UserReadPtr<Iovec>, usize) = self.cx.into();
        let file = self
            .alive_then(move |a| a.fd_table.get(Fd::new(fd)).cloned())
            .ok_or(SysError::EBADF)?;
        if !file.writable() {
            return Err(SysError::EPERM);
        }
        let uc = UserCheck::new(self.process);
        let vbuf = uc.readonly_slice(iov, vlen).await?;
        let mut cnt = 0;
        for &Iovec { iov_base, iov_len } in vbuf.access().iter() {
            let buf = uc.readonly_slice(iov_base, iov_len).await?;
            cnt += file.write(&*buf.access()).await?;
        }
        Ok(cnt)
    }
    pub async fn sys_pread64(&mut self) -> SysRet {
        stack_trace!();
        if PRINT_SYSCALL_FS {
            println!("sys_pread64");
        }
        let (fd, buf, len, offset): (usize, UserWritePtr<u8>, usize, usize) = self.cx.into();
        let buf = UserCheck::new(self.process)
            .writable_slice(buf, len)
            .await?;
        let file = self
            .alive_then(move |a| a.fd_table.get(Fd::new(fd)).cloned())
            .ok_or(SysError::EBADF)?;
        if !file.readable() {
            return Err(SysError::EPERM);
        }
        file.read_at(offset, &mut *buf.access_mut()).await
    }
    pub async fn sys_sendfile(&mut self) -> SysRet {
        stack_trace!();
        let (out_fd, in_fd, offset, count): (Fd, Fd, UserInOutPtr<usize>, usize) = self.cx.into();
        if PRINT_SYSCALL_FS {
            println!(
                "sys_sendfile out: {:?} in: {:?} off:{:#x} n:{}",
                out_fd,
                in_fd,
                offset.as_usize(),
                count
            );
        }
        let (out_file, in_file) = match self.alive_then(|a| {
            (
                a.fd_table.get(out_fd).cloned(),
                a.fd_table.get(in_fd).cloned(),
            )
        }) {
            (Some(out_file), Some(in_file)) => (out_file, in_file),
            _ => return Err(SysError::EBADF),
        };
        let offset = UserCheck::new(self.process)
            .writable_value_nullable(offset)
            .await?;
        let mut buf = Vec::new();
        buf.resize(count, 0);
        if let Some(offset) = offset {
            let off = offset.load();
            let n = in_file.read_at(off, &mut *buf).await?;
            out_file.write(&mut buf[..n]).await?;
            offset.store(off + n);
            Ok(n)
        } else {
            let n = in_file.read(&mut buf[..]).await?;
            out_file.write(&mut buf[..n]).await?;
            Ok(n)
        }
    }
    pub async fn sys_readlinkat(&mut self) -> SysRet {
        stack_trace!();
        let (fd, path, buf, size): (isize, UserReadPtr<u8>, UserWritePtr<u8>, usize) =
            self.cx.into();
        let inode = self
            .fd_path_open(fd, path, OpenFlags::RDONLY, Mode(0o600))
            .await?;
        let path = inode.path_str();
        let plen = path.iter().fold(0, |a, s| a + s.len() + 1).max(1) + 1;
        let dst = UserCheck::new(self.process)
            .writable_slice(buf, size)
            .await?;
        if plen >= dst.len() {
            return Err(SysError::ENAMETOOLONG);
        }
        path::write_path_to(path.iter().map(|s| s.as_ref()), &mut *dst.access_mut());
        Ok(plen.min(size))
    }

    pub async fn sys_mkdirat(&mut self) -> SysRet {
        stack_trace!();
        let (fd, path, mode): (isize, UserReadPtr<u8>, Mode) = self.cx.into();
        if PRINT_SYSCALL_FS {
            println!("sys_mkdirat {} {:#x} {:#x}", fd, path.as_usize(), mode.0);
        }
        let flags = OpenFlags::RDWR | OpenFlags::CREAT | OpenFlags::DIRECTORY;

        self.fd_path_create_any(fd, path, flags, mode).await?;
        Ok(0)
    }
    pub async fn sys_unlinkat(&mut self) -> SysRet {
        stack_trace!();
        let (fd, path, flags): (isize, UserReadPtr<u8>, u32) = self.cx.into();
        if PRINT_SYSCALL_FS {
            println!("sys_unlinkat flags: {}", flags);
        }
        let (base, path) = self.fd_path_impl(fd, path).await?;
        let dir = flags & AT_REMOVEDIR as u32 != 0;
        fs::unlinkat((base, &path), dir).await?;
        Ok(0)
    }
    pub async fn sys_faccessat(&mut self) -> SysRet {
        stack_trace!();
        let (fd, path, mode, _flags): (isize, UserReadPtr<u8>, Mode, u32) = self.cx.into();
        let _inode = self.fd_path_open(fd, path, OpenFlags::RDONLY, mode).await?;
        Ok(0)
    }
    pub async fn sys_chdir(&mut self) -> SysRet {
        stack_trace!();
        let path: UserReadPtr<u8> = self.cx.para1();
        let path = UserCheck::new(self.process)
            .array_zero_end(path)
            .await?
            .to_vec();
        let path = String::from_utf8(path)?;
        let flags = OpenFlags::RDONLY | OpenFlags::DIRECTORY;

        let inode = fs::open_file(
            (Ok(self.alive_then(|a| a.cwd.clone())), path.as_str()),
            flags,
            Mode(0o600),
        )
        .await?;
        self.alive_then(|a| a.cwd = inode);
        Ok(0)
    }
    // changes the ownership of the file referred to by the open file descriptor fd.
    pub fn sys_fchown(&mut self) -> SysRet {
        Ok(0)
    }
    pub async fn sys_openat(&mut self) -> SysRet {
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
        let flags = OpenFlags::from_bits(flags).unwrap();
        let inode = self.fd_path_open(fd, path, flags, mode).await?;
        let close_on_exec = flags.contains(OpenFlags::CLOEXEC);
        let fd = self.alive_then(|a| a.fd_table.insert(inode, close_on_exec, flags))?;
        Ok(fd.0)
    }
    pub fn sys_close(&mut self) -> SysRet {
        stack_trace!();
        let fd = self.cx.para1();
        if PRINT_SYSCALL_FS {
            println!("sys_close fd: {}", fd);
        }
        let fd = Fd::new(fd);
        let file = self
            .alive_then(move |a| a.fd_table.remove(fd))
            .ok_or(SysError::EBADF)?;
        drop(file); // just for clarity
        Ok(0)
    }
    /// 管道的读端只有当管道中无数据时才会阻塞, 如果存在数据则必然返回, 即使读取的数量没有达到要求
    pub async fn sys_pipe2(&mut self) -> SysRet {
        stack_trace!();
        let (pipe, flags): (UserWritePtr<[u32; 2]>, u32) = self.cx.into();
        if PRINT_SYSCALL_FS {
            println!("sys_pipe2 pipe: {:#x} flags: {:#x}", pipe.as_usize(), flags);
        }
        let write_to = UserCheck::new(self.process).writable_slice(pipe, 1).await?;
        let flags = OpenFlags::from_bits_truncate(flags);
        let close_on_exec = flags.contains(OpenFlags::CLOEXEC);
        if flags.contains(OpenFlags::DIRECT | OpenFlags::NONBLOCK) {
            unimplemented!();
        }
        let (reader, writer) = pipe::make_pipe()?;
        let (rfd, wfd) = self.alive_then(move |a| -> SysR<_> {
            let rfd = a.fd_table.insert(reader, close_on_exec, flags)?.to_usize();
            let wfd = a.fd_table.insert(writer, close_on_exec, flags)?.to_usize();
            Ok((rfd as u32, wfd as u32))
        })?;
        write_to.store([rfd, wfd]);
        Ok(0)
    }
    pub fn sys_fcntl(&mut self) -> SysRet {
        let (fd, cmd, arg): (usize, u32, usize) = self.cx.into();
        if PRINT_SYSCALL_FS {
            println!("sys_fcntl fd: {} cmd: {} arg: {}", fd, cmd, arg);
        }
        self.alive_then(|a| a.fd_table.fcntl(Fd(fd), cmd, arg))
    }
    pub fn sys_ioctl(&mut self) -> SysRet {
        stack_trace!();
        let (fd, cmd, arg): (usize, u32, usize) = self.cx.into();
        if PRINT_SYSCALL_FS {
            println!("sys_ioctl fd: {} cmd: {} arg: {}", fd, cmd, arg);
        }
        self.alive_then(|a| a.fd_table.get(Fd(fd)).cloned())
            .ok_or(SysError::ENFILE)?
            .ioctl(cmd, arg)
    }
    pub async fn sys_syslog(&mut self) -> SysRet {
        stack_trace!();
        let (ty, buf, len): (u32, UserWritePtr<u8>, usize) = self.cx.into();
        if PRINT_SYSCALL_FS {
            println!(
                "sys_syslog (noimplement) ty: {} buf: {:#x} len: {}",
                ty,
                buf.as_usize(),
                len
            );
        }
        Ok(0)
    }
    pub async fn sys_renameat2(&mut self) -> SysRet {
        stack_trace!();
        let (odfd, opath, ndfd, npath, _flags): (
            isize,
            UserReadPtr<u8>,
            isize,
            UserReadPtr<u8>,
            u32,
        ) = self.cx.into();
        let old = self
            .fd_path_open(odfd, opath, OpenFlags::empty(), Mode(0o600))
            .await?;
        if old.is_dir() {
            // unimplemented!();
            return Err(SysError::EINVAL);
        }
        let new = self
            .fd_path_create_any(ndfd, npath, OpenFlags::CREAT, Mode(0o600))
            .await?;
        let len = old.read_all().await?;
        drop(old);
        new.write(&len[..]).await?;
        let (base, path) = self.fd_path_impl(odfd, opath).await?;
        fs::unlinkat((base, &path), false).await?;
        Ok(0)
    }
}
