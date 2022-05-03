use alloc::sync::Arc;
use ftl_util::error::SysError;

use super::OpenFlags;

mod dentry;
pub(super) mod inode;
mod manager;
mod mount;
mod super_block;

pub use self::inode::VfsInode;

pub async fn init() {
    inode::init().await;
}
