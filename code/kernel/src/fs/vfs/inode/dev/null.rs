use alloc::{
    boxed::Box,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use ftl_util::{
    async_tools::AsyncFile,
    error::SysError,
    fs::{
        stat::{Stat, S_IFCHR},
        DentryType, File, VfsInode,
    },
};

use crate::tools::xasync::Async;

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
    fn stat<'a>(&'a self, stat: &'a mut Stat) -> Async<'a, Result<(), SysError>> {
        Box::pin(async move {
            stat.st_mode = S_IFCHR | 0o666;
            Ok(())
        })
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
