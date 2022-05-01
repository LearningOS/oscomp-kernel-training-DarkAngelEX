//! 类似linux的虚拟文件系统

pub mod pipe;
mod stdio;
mod vfs;

pub use self::{
    stdio::{Stdin, Stdout},
    vfs::{list_apps, open_file, VfsInode},
};

use crate::{
    syscall::{SysError, SysResult, UniqueSysError},
    tools::xasync::Async,
    user::{UserData, UserDataMut},
};

bitflags! {
    pub struct OpenFlags: u32 {
        const ACCMODE   = 00000003;
        const RDONLY    = 00000000;
        const WRONLY    = 00000001;
        const RDWR      = 00000002;
        const CREAT     = 00000100; // 不存在则创建
        const EXCL      = 00000200; //
        const NOCTTY    = 00000400; //
        const TRUNC     = 00001000; // 文件清空 ultra os is 2000 ???
        const APPEND    = 00002000;
        const NONBLOCK  = 00004000;
        const DSYNC     = 00010000;
        const FASYNC    = 00020000;
        const DIRECT    = 00040000;
        const LARGEFILE = 00100000;
        const DIRECTORY = 00200000;
        const NOFOLLOW  = 00400000;
        const NOATIME   = 01000000;
        const CLOEXEC   = 02000000;
    }
}

impl OpenFlags {
    /// Do not check validity for simplicity
    /// Return (readable, writable)
    pub fn read_write(
        &self,
    ) -> Result<(bool, bool), UniqueSysError<{ SysError::EINVAL as isize }>> {
        use core::ops::BitAnd;
        let v = match self.bitand(Self::ACCMODE) {
            Self::RDONLY => (true, false),
            Self::WRONLY => (false, true),
            Self::RDWR => (true, true),
            _ => return Err(UniqueSysError),
        };
        Ok(v)
    }
}

pub type AsyncFile<'a> = Async<'a, Result<usize, SysError>>;

pub trait File: Send + Sync + 'static {
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
    fn read_at(&self, _offset: usize, _write_only: UserDataMut<u8>) -> AsyncFile {
        unimplemented!()
    }
    fn write_at(&self, _offset: usize, _read_only: UserData<u8>) -> AsyncFile {
        unimplemented!()
    }
    fn read(&self, write_only: UserDataMut<u8>) -> AsyncFile;
    fn write(&self, read_only: UserData<u8>) -> AsyncFile;
    fn ioctl(&self, _cmd: u32, _arg: usize) -> SysResult {
        Ok(0)
    }
}

pub async fn init() {
    vfs::init().await;
}
