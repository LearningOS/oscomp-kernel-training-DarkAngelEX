mod inode;
pub mod pipe;
mod stdio;

use core::{future::Future, pin::Pin};

pub use self::{
    inode::{list_apps, open_file, OSInode, OpenFlags},
    stdio::{Stdin, Stdout},
};
use alloc::{boxed::Box, sync::Arc};

use crate::{
    process::Process,
    syscall::SysError,
    user::{UserData, UserDataMut},
};

pub type AsyncFileOutput = Pin<Box<dyn Future<Output = Result<usize, SysError>> + Send + 'static>>;
pub trait File: Send + Sync + 'static {
    fn readable(&self) -> bool;
    fn writable(&self) -> bool;
    fn read(self: Arc<Self>, proc: Arc<Process>, write_only: UserDataMut<u8>) -> AsyncFileOutput;
    fn write(self: Arc<Self>, proc: Arc<Process>, read_only: UserData<u8>) -> AsyncFileOutput;
}

pub fn init() {
    inode::init();
}
