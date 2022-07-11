use alloc::{
    boxed::Box,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use ftl_util::{
    async_tools::AsyncFile,
    error::SysError,
    fs::{DentryType, File, VfsInode},
};

use crate::tools::xasync::Async;

pub struct ZeroInode;

static mut ZERO_INODE: Option<Arc<ZeroInode>> = None;
static mut ZERO_PATH: Vec<String> = Vec::new();

pub fn init() {
    unsafe {
        ZERO_PATH.push("dev".to_string());
        ZERO_PATH.push("zero".to_string());
        ZERO_INODE = Some(Arc::new(ZeroInode));
    }
}

pub fn inode() -> Arc<ZeroInode> {
    unsafe { ZERO_INODE.as_ref().unwrap().clone() }
}

impl File for ZeroInode {
    fn to_vfs_inode(&self) -> Result<&dyn VfsInode, SysError> {
        Ok(self)
    }
    fn readable(&self) -> bool {
        true
    }
    fn writable(&self) -> bool {
        true
    }
    fn read<'a>(&'a self, write_only: &'a mut [u8]) -> AsyncFile {
        Box::pin(async move {
            write_only.fill(0);
            Ok(write_only.len())
        })
    }
    fn write<'a>(&'a self, read_only: &'a [u8]) -> AsyncFile {
        Box::pin(async move { Ok(read_only.len()) })
    }
}

impl VfsInode for ZeroInode {
    fn read_all(&self) -> Async<Result<Vec<u8>, SysError>> {
        Box::pin(async move { Err(SysError::EPERM) })
    }

    fn list(&self) -> Async<Result<Vec<(DentryType, String)>, SysError>> {
        Box::pin(async move { Err(SysError::ENOTDIR) })
    }

    fn path(&self) -> &[String] {
        unsafe { &ZERO_PATH }
    }
}
