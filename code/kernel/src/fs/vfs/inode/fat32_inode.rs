use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use alloc::{boxed::Box, sync::Arc, vec::Vec};
use fat32::Fat32Manager;
use ftl_util::{error::SysError, utc_time::UtcTime};

use crate::{
    drivers,
    fs::{AsyncFile, File, OpenFlags},
    user::{UserData, UserDataMut},
};

pub use fat32::AnyInode;

pub struct Fat32Inode {
    readable: AtomicBool,
    writable: AtomicBool,
    ptr: AtomicUsize,
    inode: AnyInode,
}

impl Fat32Inode {
    pub async fn read_all(&self) -> Vec<u8> {
        todo!()
    }
}

static mut MANAGER: Option<Fat32Manager> = None;

fn manager() -> &'static Fat32Manager {
    unsafe { MANAGER.as_ref().unwrap() }
}

pub async fn init() {
    unsafe {
        MANAGER = Some(Fat32Manager::new(100, 200, 100, 200, 100));
        let manager = MANAGER.as_mut().unwrap();
        manager
            .init(drivers::device().clone(), Box::new(|| UtcTime::base()))
            .await;
    }
    todo!();
}

pub async fn list_apps() {
    println!("/**** APPS ****");
    for app in manager().root_dir().list(manager()).await.unwrap() {
        println!("{}", app);
    }
    println!("**************/");
}

pub async fn open_file(name: &str, flags: OpenFlags) -> Result<Arc<Fat32Inode>, SysError> {
    let root = manager().root_dir();
    todo!()
}

impl File for Fat32Inode {
    fn readable(&self) -> bool {
        self.readable.load(Ordering::Relaxed)
    }
    fn writable(&self) -> bool {
        self.writable.load(Ordering::Relaxed)
    }
    fn read_at(&self, offset: usize, buf: UserDataMut<u8>) -> AsyncFile {
        Box::pin(async move {
            let inode = match &self.inode {
                AnyInode::Dir(_) => return Err(SysError::EISDIR),
                AnyInode::File(inode) => inode,
            };
            let n = inode
                .read_at(manager(), offset, &mut *buf.access_mut())
                .await?;
            Ok(n)
        })
    }
    fn write_at(&self, offset: usize, buf: UserData<u8>) -> AsyncFile {
        Box::pin(async move {
            let inode = match &self.inode {
                AnyInode::Dir(_) => return Err(SysError::EISDIR),
                AnyInode::File(inode) => inode,
            };
            let n = inode.write_at(manager(), offset, &*buf.access()).await?;
            Ok(n)
        })
    }
    fn read(&self, buf: UserDataMut<u8>) -> AsyncFile {
        Box::pin(async move {
            let offset = self.ptr.load(Ordering::Acquire);
            let inode = match &self.inode {
                AnyInode::Dir(_) => return Err(SysError::EISDIR),
                AnyInode::File(inode) => inode,
            };
            let n = inode
                .read_at(manager(), offset, &mut *buf.access_mut())
                .await?;
            self.ptr.store(offset + n, Ordering::Release);
            Ok(n)
        })
    }
    fn write(&self, buf: UserData<u8>) -> AsyncFile {
        Box::pin(async move {
            let offset = self.ptr.load(Ordering::Acquire);
            let inode = match &self.inode {
                AnyInode::Dir(_) => return Err(SysError::EISDIR),
                AnyInode::File(inode) => inode,
            };
            let n = inode.write_at(manager(), offset, &*buf.access()).await?;
            self.ptr.store(offset + n, Ordering::Release);
            Ok(n)
        })
    }
}
