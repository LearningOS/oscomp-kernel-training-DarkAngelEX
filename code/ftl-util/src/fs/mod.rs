pub mod stat;

use alloc::{boxed::Box, string::String, vec::Vec};

use crate::{
    async_tools::{Async, AsyncFile},
    error::{SysError, SysResult},
    time::TimeSpec,
};

use self::stat::Stat;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DentryType {
    UNKNOWN = 0,
    FIFO = 1,  // pipe
    CHR = 2,   // character device
    DIR = 4,   // directory
    BLK = 6,   // block device
    REG = 8,   // regular file
    LNK = 10,  // symbolic link
    SOCK = 12, // UNIX domain socket
}

pub enum Seek {
    Set,
    Cur,
    End,
}

impl Seek {
    pub fn from_user(v: u32) -> Result<Self, SysError> {
        const SEEK_SET: u32 = 0;
        const SEEK_CUR: u32 = 1;
        const SEEK_END: u32 = 2;
        match v {
            SEEK_SET => Ok(Self::Set),
            SEEK_CUR => Ok(Self::Cur),
            SEEK_END => Ok(Self::End),
            _ => Err(SysError::EINVAL),
        }
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
        const DIRECTORY = 0o0200000;
        const NOFOLLOW  = 0o0400000;
        const NOATIME   = 0o1000000;
        const CLOEXEC   = 0o2000000;
    }
}

impl OpenFlags {
    /// Do not check validity for simplicity
    /// Return (readable, writable)
    pub fn read_write(self) -> Result<(bool, bool), SysError> {
        use core::ops::BitAnd;
        let v = match self.bitand(Self::ACCMODE) {
            Self::RDONLY => (true, false),
            Self::WRONLY => (false, true),
            Self::RDWR => (true, true),
            _ => return Err(SysError::EINVAL),
        };
        Ok(v)
    }
    pub fn create(self) -> bool {
        self.contains(Self::CREAT)
    }
    pub fn dir(self) -> bool {
        self.contains(Self::DIRECTORY)
    }
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

pub trait VfsInode: File {
    fn read_all(&self) -> Async<Result<Vec<u8>, SysError>>;
    fn list(&self) -> Async<Result<Vec<(DentryType, String)>, SysError>>;
    fn path(&self) -> &[String];
}

impl dyn VfsInode {
    pub fn path_iter(
        &self,
    ) -> impl DoubleEndedIterator<Item = &str> + ExactSizeIterator<Item = &str> {
        self.path().iter().map(|s| s.as_str())
    }
}

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
    fn lseek(&self, _offset: isize, _whence: Seek) -> SysResult {
        unimplemented!("lseek {}", core::any::type_name::<Self>())
    }
    fn read_at<'a>(&'a self, _offset: usize, _buf: &'a mut [u8]) -> AsyncFile {
        unimplemented!("read_at {}", core::any::type_name::<Self>())
    }
    fn write_at<'a>(&'a self, _offset: usize, _buf: &'a [u8]) -> AsyncFile {
        unimplemented!("write_at {}", core::any::type_name::<Self>())
    }
    fn read<'a>(&'a self, write_only: &'a mut [u8]) -> AsyncFile;
    fn write<'a>(&'a self, read_only: &'a [u8]) -> AsyncFile;
    fn ioctl(&self, _cmd: u32, _arg: usize) -> SysResult {
        Ok(0)
    }
    fn stat<'a>(&'a self, _stat: &'a mut Stat) -> Async<'a, Result<(), SysError>> {
        Box::pin(async move { Err(SysError::EACCES) })
    }
    fn utimensat(&self, _times: [TimeSpec; 2]) -> Async<SysResult> {
        unimplemented!("utimensat {}", core::any::type_name::<Self>())
    }
}
