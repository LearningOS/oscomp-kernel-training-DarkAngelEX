mod inode;
pub mod pipe;
mod stdio;

pub use self::{
    inode::{list_apps, open_file, OSInode, OpenFlags},
    stdio::{Stdin, Stdout},
};
use alloc::sync::Arc;

use crate::{
    syscall::SysError,
    user::{UserData, UserDataMut}, tools::xasync::Async,
};

pub type AsyncFile = Async<Result<usize, SysError>>;
pub trait File: Send + Sync + 'static {
    fn readable(&self) -> bool;
    fn writable(&self) -> bool;
    fn read(self: Arc<Self>, write_only: UserDataMut<u8>) -> AsyncFile;
    fn write(self: Arc<Self>, read_only: UserData<u8>) -> AsyncFile;
}

pub fn init() {
    inode::init();
}
