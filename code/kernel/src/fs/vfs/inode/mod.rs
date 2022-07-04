use crate::{
    fs::{File, Mode, OpenFlags},
    syscall::SysError,
    tools::{path, xasync::Async},
};
use alloc::{string::String, sync::Arc, vec::Vec};

use ftl_util::fs::DentryType;

// type InodeImpl = easyfs_inode::EasyFsInode;
type InodeImpl = fat32_inode::Fat32Inode;

pub mod dev;
mod easyfs_inode;
mod fat32_inode;

pub trait VfsInode: File {
    fn read_all(&self) -> Async<Result<Vec<u8>, SysError>>;
    fn list(&self) -> Async<Result<Vec<(DentryType, String)>, SysError>>;
    fn path(&self) -> &[String];
}

impl dyn VfsInode {
    pub fn path_iter(
        &self,
    ) -> impl DoubleEndedIterator<Item = &str> + ExactSizeIterator<Item = &str> {
        self.path().iter().map(|s| s.as_str())
    }
}

pub async fn init() {
    dev::init();
    fat32_inode::init().await;
}

pub async fn list_apps() {
    fat32_inode::list_apps().await
}

pub async fn create_any<'a>(
    base: Option<Result<impl Iterator<Item = &'a str>, SysError>>,
    path: &'a str,
    flags: OpenFlags,
    _mode: Mode,
) -> Result<(), SysError> {
    stack_trace!();
    fat32_inode::create_any(base, path, flags).await
}
pub async fn open_file<'a>(
    base: Option<Result<impl Iterator<Item = &'a str>, SysError>>,
    path: &'a str,
    flags: OpenFlags,
    _mode: Mode,
) -> Result<Arc<dyn VfsInode>, SysError> {
    stack_trace!();
    let mut stack = Vec::new();
    match path.as_bytes().first() {
        Some(b'/') => (),
        _ => path::walk_iter_path(base.unwrap()?, &mut stack),
    }
    path::walk_path(path, &mut stack);
    let inode = match stack.split_first() {
        Some((&"dev", path)) => dev::open_file(path)?,
        _ => fat32_inode::open_file(&stack, flags).await?,
    };
    Ok(inode)
}
pub async fn open_file_abs<'a>(
    path: &'a str,
    flags: OpenFlags,
    _mode: Mode,
) -> Result<Arc<dyn VfsInode>, SysError> {
    stack_trace!();
    let mut stack = Vec::new();
    match path.as_bytes().first() {
        Some(b'/') => (),
        _ => panic!(),
    }
    path::walk_path(path, &mut stack);
    let inode = match stack.split_first() {
        Some((&"dev", path)) => dev::open_file(path)?,
        _ => fat32_inode::open_file(&stack, flags).await?,
    };
    Ok(inode)
}

pub async fn unlink<'a>(
    base: Option<Result<impl Iterator<Item = &'a str>, SysError>>,
    path: &'a str,
    flags: OpenFlags,
) -> Result<(), SysError> {
    stack_trace!();
    fat32_inode::unlink(base, path, flags).await
}
