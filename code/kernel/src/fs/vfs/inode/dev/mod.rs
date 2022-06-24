use alloc::sync::Arc;
use ftl_util::error::SysError;

use super::VfsInode;

mod null;
mod tty;
mod zero;

pub fn init() {
    null::init();
    tty::init();
    zero::init();
}

pub fn open_file(path: &[&str]) -> Result<Arc<dyn VfsInode>, SysError> {
    let inode: Arc<dyn VfsInode> = match path {
        ["null"] => null::inode(),
        ["tty"] => tty::inode(),
        ["zero"] => zero::inode(),
        _ => return Err(SysError::ENOENT),
    };
    Ok(inode)
}
