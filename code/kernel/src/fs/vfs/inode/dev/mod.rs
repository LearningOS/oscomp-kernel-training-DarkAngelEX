use alloc::sync::Arc;
use ftl_util::error::SysError;

use crate::fs::{Mode, OpenFlags};

use super::VfsInode;

pub mod null;
pub mod shm;
pub mod tty;
pub mod zero;

pub fn init() {
    null::init();
    shm::init();
    tty::init();
    zero::init();
}

pub async fn open_file(
    path: &[&str],
    flags: OpenFlags,
    mode: Mode,
) -> Result<Arc<dyn VfsInode>, SysError> {
    let inode: Arc<dyn VfsInode> = match path.split_first() {
        Some((&"tty", [])) => tty::inode(),
        Some((&"null", [])) => null::inode(),
        Some((&"zero", [])) => zero::inode(),
        Some((&"shm", path)) => shm::open_file(path, flags, mode).await?,
        _ => return Err(SysError::ENOENT),
    };
    Ok(inode)
}

pub async fn unlink(path: &[&str], flags: OpenFlags) -> Result<(), SysError> {
    stack_trace!();
    match path.split_first() {
        Some((&"shm", path)) => shm::unlink(path, flags).await,
        _ => Err(SysError::ENOENT),
    }
}
