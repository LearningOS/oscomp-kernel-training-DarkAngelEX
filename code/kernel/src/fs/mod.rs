//! 类似linux的虚拟文件系统

pub mod pipe;
pub mod stat;
mod stdio;
mod vfs;

use alloc::boxed::Box;

use self::stat::Stat;
pub use self::{
    stdio::{Stdin, Stdout},
    vfs::inode::{create_any, list_apps, open_file, unlink, VfsInode},
};

use crate::{
    syscall::{SysError, SysResult, UniqueSysError},
    tools::xasync::Async,
};

pub async fn init() {
    vfs::init().await;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mode(pub u32);

impl Mode {
    pub const MASK: u32 = 0b111;
    pub fn split(x: u32) -> (bool, bool, bool) {
        (x & 0b100 != 0, x & 0b010 != 0, x & 0b001 != 0)
    }
    pub fn user(self) -> (bool, bool, bool) {
        Self::split((self.0 >> 6) & Self::MASK)
    }
    pub fn group(self) -> (bool, bool, bool) {
        Self::split((self.0 >> 3) & Self::MASK)
    }
    pub fn other(self) -> (bool, bool, bool) {
        Self::split(self.0 & Self::MASK)
    }
    pub fn user_v(self) -> u32 {
        self.0 & (Self::MASK << 6)
    }
    pub fn group_v(self) -> u32 {
        self.0 & (Self::MASK << 3)
    }
    pub fn other_v(self) -> u32 {
        self.0 & Self::MASK
    }
}

bitflags! {
    pub struct OpenFlags: u32 {
        const ACCMODE   = 0o0000003;
        const RDONLY    = 0o0000000;
        const WRONLY    = 0o0000001;
        const RDWR      = 0o0000002;
        const CREAT     = 0o0000100; // LINUX 不存在则创建, 存在则删除再创建
        const EXCL      = 0o0000200; //
        const NOCTTY    = 0o0000400; //
        const TRUNC     = 0o0001000; // 文件清空 ultra os is 2000 ???
        const APPEND    = 0o0002000;
        const NONBLOCK  = 0o0004000;
        const DSYNC     = 0o0010000;
        const FASYNC    = 0o0020000;
        const DIRECT    = 0o0040000;
        const LARGEFILE = 0o0100000;
        // const DIRECTORY = 00200000; // LINUX
        const DIRECTORY = 0x0200000; // test run
        const NOFOLLOW  = 0o0400000;
        const NOATIME   = 0o1000000;
        const CLOEXEC   = 0o2000000;
    }
}

impl OpenFlags {
    /// Do not check validity for simplicity
    /// Return (readable, writable)
    pub fn read_write(self) -> Result<(bool, bool), UniqueSysError<{ SysError::EINVAL as isize }>> {
        use core::ops::BitAnd;
        let v = match self.bitand(Self::ACCMODE) {
            Self::RDONLY => (true, false),
            Self::WRONLY => (false, true),
            Self::RDWR => (true, true),
            _ => return Err(UniqueSysError),
        };
        Ok(v)
    }
    fn create(self) -> bool {
        self.contains(Self::CREAT)
    }
    fn dir(self) -> bool {
        self.contains(Self::DIRECTORY)
    }
}

pub type AsyncFile<'a> = Async<'a, Result<usize, SysError>>;

pub trait File: Send + Sync + 'static {
    // 这个文件的工作路径
    fn to_vfs_inode(&self) -> Result<&dyn VfsInode, SysError> {
        Err(SysError::ENOTDIR)
    }
    fn readable(&self) -> bool;
    fn writable(&self) -> bool;
    fn can_mmap(&self) -> bool {
        self.can_read_offset() && self.can_write_offset()
    }
    fn can_read_offset(&self) -> bool {
        false
    }
    fn can_write_offset(&self) -> bool {
        false
    }
    fn read_at<'a>(&'a self, _offset: usize, _buf: &'a mut [u8]) -> AsyncFile {
        unimplemented!()
    }
    fn write_at<'a>(&'a self, _offset: usize, _buf: &'a [u8]) -> AsyncFile {
        unimplemented!()
    }
    fn read<'a>(&'a self, write_only: &'a mut [u8]) -> AsyncFile;
    fn write<'a>(&'a self, read_only: &'a [u8]) -> AsyncFile;
    fn ioctl(&self, _cmd: u32, _arg: usize) -> SysResult {
        Ok(0)
    }
    fn stat<'a>(&'a self, _stat: &'a mut Stat) -> Async<'a, Result<(), SysError>> {
        Box::pin(async move { Err(SysError::EACCES) })
    }
}
