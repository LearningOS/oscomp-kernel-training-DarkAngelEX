//! 类似linux的虚拟文件系统

pub mod pipe;
mod stdio;
mod vfs;

pub use self::{
    stdio::{Stdin, Stdout},
    vfs::inode::{create_any, dev, list_apps, open_file, open_file_abs, unlink},
};

use crate::memory::user_ptr::UserInOutPtr;

pub async fn init() {
    vfs::init().await;
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Iovec {
    pub iov_base: UserInOutPtr<u8>,
    pub iov_len: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Pollfd {
    pub fd: u32,
    pub events: u16,
    pub revents: u16,
}
