mod inode;
pub mod pipe;
mod stdio;

pub use self::{
    inode::{list_apps, open_file, OSInode, OpenFlags},
    stdio::{Stdin, Stdout},
};
use alloc::sync::Arc;

use crate::{
    syscall::{SysError, SysResult},
    tools::xasync::Async,
    user::{UserData, UserDataMut},
};

pub type AsyncFile = Async<Result<usize, SysError>>;
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
    fn read_at(self: Arc<Self>, _offset: usize, _write_only: UserDataMut<u8>) -> AsyncFile {
        unimplemented!()
    }
    fn write_at(self: Arc<Self>, _offset: usize, _read_only: UserData<u8>) -> AsyncFile {
        unimplemented!()
    }
    fn read(self: Arc<Self>, write_only: UserDataMut<u8>) -> AsyncFile;
    fn write(self: Arc<Self>, read_only: UserData<u8>) -> AsyncFile;
    fn ioctl(&self, _cmd: u32, _arg: usize) -> SysResult {
        Ok(0)
    }
}

pub fn init() {
    inode::init();
}
