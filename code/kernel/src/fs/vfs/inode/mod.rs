use crate::tools::path;
use alloc::{sync::Arc, vec::Vec};

use ftl_util::{
    error::SysR,
    fs::{Mode, OpenFlags, VfsInode},
};

// type InodeImpl = easyfs_inode::EasyFsInode;
type InodeImpl = fat32_inode::Fat32Inode;

pub mod dev;
mod fat32_inode;

pub async fn init() {
    dev::init();
    fat32_inode::init().await;
}

pub async fn list_apps() {
    fat32_inode::list_apps().await
}

pub async fn create_any<'a>(
    base: Option<SysR<impl Iterator<Item = &'a str>>>,
    path: &'a str,
    flags: OpenFlags,
    _mode: Mode,
) -> SysR<()> {
    stack_trace!();
    let mut stack = Vec::new();
    match path.as_bytes().first() {
        Some(b'/') => (),
        _ => path::walk_iter_path(base.unwrap()?, &mut stack),
    }
    path::walk_path(path, &mut stack);
    fat32_inode::create_any(&stack, flags).await
}
pub async fn open_file<'a>(
    base: Option<SysR<impl Iterator<Item = &'a str>>>,
    path: &'a str,
    flags: OpenFlags,
    mode: Mode,
) -> SysR<Arc<dyn VfsInode>> {
    stack_trace!();
    let mut stack = Vec::new();
    match path.as_bytes().first() {
        Some(b'/') => (),
        _ => path::walk_iter_path(base.unwrap()?, &mut stack),
    }
    path::walk_path(path, &mut stack);
    let inode = match stack.split_first() {
        Some((&"dev", path)) => dev::open_file(path, flags, mode).await?,
        _ => fat32_inode::open_file(&stack, flags).await?,
    };
    Ok(inode)
}
pub async fn open_file_abs<'a>(
    path: &'a str,
    flags: OpenFlags,
    mode: Mode,
) -> SysR<Arc<dyn VfsInode>> {
    stack_trace!();
    let mut stack = Vec::new();
    match path.as_bytes().first() {
        Some(b'/') => (),
        _ => panic!(),
    }
    path::walk_path(path, &mut stack);
    let inode = match stack.split_first() {
        Some((&"dev", path)) => dev::open_file(path, flags, mode).await?,
        _ => fat32_inode::open_file(&stack, flags).await?,
    };
    Ok(inode)
}

pub async fn unlink<'a>(
    base: Option<SysR<impl Iterator<Item = &'a str>>>,
    path: &'a str,
    flags: OpenFlags,
) -> SysR<()> {
    stack_trace!();
    let mut stack = Vec::new();
    match path.as_bytes().first() {
        Some(b'/') => (),
        _ => path::walk_iter_path(base.unwrap()?, &mut stack),
    }
    path::walk_path(path, &mut stack);
    match stack.split_first() {
        Some((&"dev", path)) => dev::unlink(path, flags).await,
        _ => fat32_inode::unlink(&stack, flags).await,
    }
}
