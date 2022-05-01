use alloc::sync::Arc;
use ftl_util::error::SysError;

pub use self::inode::VfsInode;

use super::OpenFlags;

mod dentry;
mod inode;
mod manager;
mod mount;
mod super_block;

pub fn init() {
    inode::init();
}

pub fn list_apps() {
    inode::list_apps()
}

pub fn open_file(name: &str, flags: OpenFlags) -> Result<Arc<VfsInode>, SysError> {
    inode::open_file(name, flags)
}
