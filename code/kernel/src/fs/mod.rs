mod inode;
mod stdio;
pub mod pipe;

use core::{future::Future, pin::Pin};

use alloc::{boxed::Box, sync::Arc};
pub use inode::{list_apps, open_file, OSInode, OpenFlags};
pub use stdio::{Stdin, Stdout};

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
