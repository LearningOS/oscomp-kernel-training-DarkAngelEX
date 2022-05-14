use crate::{
    fs::{stat::Stat, AsyncFile, File, Mode, OpenFlags},
    syscall::SysError,
    tools::xasync::Async,
    user::{UserData, UserDataMut},
};
use alloc::{string::String, sync::Arc, vec::Vec};

// type InodeImpl = easyfs_inode::EasyFsInode;
type InodeImpl = fat32_inode::Fat32Inode;

mod easyfs_inode;
mod fat32_inode;

pub struct VfsInode {
    inode: Arc<InodeImpl>,
}

pub trait FsInode {
    fn read_at(&self, _offset: usize, _write_only: UserDataMut<u8>) -> AsyncFile;
    fn write_at(&self, _offset: usize, _read_only: UserData<u8>) -> AsyncFile;
}

impl VfsInode {
    pub async fn read_all(&self) -> Result<Vec<u8>, SysError> {
        self.inode.read_all().await
    }
    pub fn path(&self) -> &[String] {
        self.inode.path()
    }
    pub fn path_iter(
        &self,
    ) -> impl DoubleEndedIterator<Item = &str> + ExactSizeIterator<Item = &str> {
        self.path().iter().map(|s| s.as_str())
    }
}

pub async fn init() {
    fat32_inode::init().await;
}

pub async fn list_apps() {
    fat32_inode::list_apps().await
}

pub async fn create_any<'a>(
    base: impl Iterator<Item = &'a str>,
    path: &'a str,
    flags: OpenFlags,
    _mode: Mode,
) -> Result<(), SysError> {
    stack_trace!();
    fat32_inode::create_any(base, path, flags).await
}
pub async fn open_file<'a>(
    base: impl Iterator<Item = &'a str>,
    path: &'a str,
    flags: OpenFlags,
    _mode: Mode,
) -> Result<Arc<VfsInode>, SysError> {
    stack_trace!();
    let inode = fat32_inode::open_file(base, path, flags).await?;
    Ok(Arc::new(VfsInode { inode }))
}

pub async fn unlink<'a>(
    base: impl Iterator<Item = &'a str>,
    path: &'a str,
    flags: OpenFlags,
) -> Result<(), SysError> {
    stack_trace!();
    fat32_inode::unlink(base, path, flags).await
}

impl File for VfsInode {
    fn to_vfs_inode(&self) -> Option<&VfsInode> {
        Some(self)
    }
    fn readable(&self) -> bool {
        self.inode.readable()
    }
    fn writable(&self) -> bool {
        self.inode.writable()
    }
    fn can_read_offset(&self) -> bool {
        self.inode.can_read_offset()
    }
    fn can_write_offset(&self) -> bool {
        self.inode.can_write_offset()
    }
    fn read_at(&self, offset: usize, write_only: UserDataMut<u8>) -> AsyncFile {
        self.inode.read_at(offset, write_only)
    }
    fn write_at(&self, offset: usize, write_only: UserData<u8>) -> AsyncFile {
        self.inode.write_at(offset, write_only)
    }
    fn read_at_kernel<'a>(&'a self, offset: usize, buf: &'a mut [u8]) -> AsyncFile {
        self.inode.read_at_kernel(offset, buf)
    }
    fn write_at_kernel<'a>(&'a self, offset: usize, buf: &'a [u8]) -> AsyncFile {
        self.inode.write_at_kernel(offset, buf)
    }
    fn read(&self, buf: UserDataMut<u8>) -> AsyncFile {
        self.inode.read(buf)
    }
    fn write(&self, buf: UserData<u8>) -> AsyncFile {
        self.inode.write(buf)
    }
    fn stat<'a>(&'a self, stat: &'a mut Stat) -> Async<'a, Result<(), SysError>> {
        self.inode.stat(stat)
    }
}
