use crate::{
    fs::{AsyncFile, File, OpenFlags, stat::Stat},
    syscall::SysError,
    user::{UserData, UserDataMut}, tools::xasync::Async,
};
use alloc::{sync::Arc, vec::Vec};

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
}

pub async fn init() {
    fat32_inode::init().await;
}

pub async fn list_apps() {
    fat32_inode::list_apps().await
}

pub async fn create_any(cwd: &str, path: &str, flags: OpenFlags) -> Result<(), SysError> {
    stack_trace!();
    fat32_inode::create_any(cwd, path, flags).await
}
pub async fn open_file(cwd: &str, path: &str, flags: OpenFlags) -> Result<Arc<VfsInode>, SysError> {
    stack_trace!();
    let inode = fat32_inode::open_file(cwd, path, flags).await?;
    Ok(Arc::new(VfsInode { inode }))
}

impl File for VfsInode {
    fn readable(&self) -> bool {
        self.inode.readable()
    }
    fn writable(&self) -> bool {
        self.inode.writable()
    }
    fn read_at(&self, offset: usize, write_only: UserDataMut<u8>) -> AsyncFile {
        self.inode.read_at(offset, write_only)
    }
    fn write_at(&self, offset: usize, write_only: UserData<u8>) -> AsyncFile {
        self.inode.write_at(offset, write_only)
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