use alloc::{
    boxed::Box,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use ftl_util::{error::SysError, fs::DentryType};

use crate::{
    fs::{AsyncFile, File, VfsInode},
    tools::xasync::Async,
};

pub struct NullInode;

static mut NULL_INODE: Option<Arc<NullInode>> = None;
static mut NULL_PATH: Vec<String> = Vec::new();

pub fn init() {
    unsafe {
        NULL_PATH.push("dev".to_string());
        NULL_PATH.push("null".to_string());
        NULL_INODE = Some(Arc::new(NullInode));
    }
}

pub fn inode() -> Arc<NullInode> {
    unsafe { NULL_INODE.as_ref().unwrap().clone() }
}

impl File for NullInode {
    fn to_vfs_inode(&self) -> Result<&dyn VfsInode, SysError> {
        Ok(self)
    }
    fn readable(&self) -> bool {
        true
    }
    fn writable(&self) -> bool {
        true
    }
    fn read<'a>(&'a self, _write_only: &'a mut [u8]) -> AsyncFile {
        Box::pin(async move { Ok(0) })
    }
    fn write<'a>(&'a self, read_only: &'a [u8]) -> AsyncFile {
        Box::pin(async move { Ok(read_only.len()) })
    }
}

impl VfsInode for NullInode {
    fn read_all(&self) -> Async<Result<Vec<u8>, SysError>> {
        Box::pin(async move { Err(SysError::EPERM) })
    }

    fn list(&self) -> Async<Result<Vec<(DentryType, String)>, SysError>> {
        Box::pin(async move { Err(SysError::ENOTDIR) })
    }

    fn path(&self) -> &[String] {
        unsafe { &NULL_PATH }
    }
}
