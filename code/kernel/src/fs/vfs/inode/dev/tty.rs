use alloc::{
    boxed::Box,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use ftl_util::{
    async_tools::AsyncFile,
    error::SysError,
    fs::{
        stat::{Stat, S_IFCHR},
        DentryType, File, Seek, VfsInode,
    },
};

use crate::{
    config::PAGE_SIZE,
    fs::{Stdin, Stdout},
    syscall::SysResult,
    tools::xasync::Async,
};

pub struct TtyInode;

static mut TTY_INODE: Option<Arc<TtyInode>> = None;
static mut TTY_PATH: Vec<String> = Vec::new();

pub fn init() {
    unsafe {
        TTY_PATH.push("dev".to_string());
        TTY_PATH.push("tty".to_string());
        TTY_INODE = Some(Arc::new(TtyInode));
    }
}

pub fn inode() -> Arc<TtyInode> {
    unsafe { TTY_INODE.as_ref().unwrap().clone() }
}

impl File for TtyInode {
    fn to_vfs_inode(&self) -> Result<&dyn VfsInode, SysError> {
        Ok(self)
    }
    fn readable(&self) -> bool {
        true
    }
    fn writable(&self) -> bool {
        true
    }
    fn lseek(&self, _offset: isize, _whence: Seek) -> SysResult {
        Err(SysError::ESPIPE)
    }
    fn read<'a>(&'a self, write_only: &'a mut [u8]) -> AsyncFile {
        Stdin.read(write_only)
    }
    fn write<'a>(&'a self, read_only: &'a [u8]) -> AsyncFile {
        Stdout.write(read_only)
    }
    fn stat<'a>(&'a self, stat: &'a mut Stat) -> Async<'a, Result<(), SysError>> {
        Box::pin(async move {
            *stat = Stat::zeroed();
            stat.st_blksize = PAGE_SIZE as u32;
            stat.st_mode = S_IFCHR | 0o666;
            Ok(())
        })
    }
}

impl VfsInode for TtyInode {
    fn read_all(&self) -> Async<Result<Vec<u8>, SysError>> {
        Box::pin(async move { Err(SysError::EPERM) })
    }

    fn list(&self) -> Async<Result<Vec<(DentryType, String)>, SysError>> {
        Box::pin(async move { Err(SysError::ENOTDIR) })
    }

    fn path(&self) -> &[String] {
        unsafe { &TTY_PATH }
    }
}
