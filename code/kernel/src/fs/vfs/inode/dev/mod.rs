use alloc::sync::Arc;
use ftl_util::error::SysError;

use super::VfsInode;

pub mod null;
pub mod tty;
pub mod zero;

pub fn open_file(path: &[&str]) -> Result<Arc<dyn VfsInode>, SysError> {
    let inode = match path {
        ["tty"] => todo!(),
        ["null"] => todo!(),
        ["zero"] => todo!(),
        _ => return Err(SysError::ENOENT),
    };
    Ok(inode)
}
