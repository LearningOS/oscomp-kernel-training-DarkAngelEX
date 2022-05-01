use alloc::sync::Arc;
use ftl_util::error::SysError;

use super::OpenFlags;

mod dentry;
mod inode;
mod manager;
mod mount;
mod super_block;

pub use self::inode::VfsInode;

pub async fn init() {
    inode::init().await;
}

pub async fn list_apps() {
    inode::list_apps().await
}

pub async fn open_file(name: &str, flags: OpenFlags) -> Result<Arc<VfsInode>, SysError> {
    inode::open_file(name, flags).await
}
