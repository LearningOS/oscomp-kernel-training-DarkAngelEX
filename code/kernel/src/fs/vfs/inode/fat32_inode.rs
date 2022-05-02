use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use alloc::{boxed::Box, sync::Arc, vec::Vec};
use fat32::Fat32Manager;
use ftl_util::{error::SysError, utc_time::UtcTime};

use crate::{
    drivers, executor,
    fs::{AsyncFile, File, OpenFlags},
    user::{AutoSie, UserData, UserDataMut},
};

pub use fat32::AnyInode;

pub struct Fat32Inode {
    readable: AtomicBool,
    writable: AtomicBool,
    ptr: AtomicUsize,
    inode: AnyInode,
}

impl Fat32Inode {
    pub async fn read_all(&self) -> Result<Vec<u8>, SysError> {
        stack_trace!();
        let buffer_size = 4096;
        let mut buffer = unsafe { Box::try_new_uninit_slice(buffer_size)?.assume_init() };
        let mut v: Vec<u8> = Vec::new();
        let file = self.inode.file().ok_or(SysError::EISDIR)?;
        let mut offset = 0;
        loop {
            let len = file.read_at(manager(), offset, &mut *buffer).await?;
            if len == 0 {
                break;
            }
            offset += len;
            v.extend_from_slice(&buffer[..len]);
        }
        v.shrink_to_fit();
        Ok(v)
    }
}

static mut MANAGER: Option<Fat32Manager> = None;

fn manager() -> &'static Fat32Manager {
    unsafe { MANAGER.as_ref().unwrap() }
}

pub async fn init() {
    stack_trace!();
    let _sie = AutoSie::new();
    unsafe {
        MANAGER = Some(Fat32Manager::new(100, 200, 100, 200, 100));
        let manager = MANAGER.as_mut().unwrap();
        manager
            .init(drivers::device().clone(), Box::new(|| UtcTime::base()))
            .await;
        manager
            .spawn_sync_task((8, 8), |f| executor::kernel_spawn(f))
            .await;
    }
}

pub async fn list_apps() {
    stack_trace!();
    let _sie = AutoSie::new();
    println!("/**** APPS ****");
    for app in manager().root_dir().list(manager()).await.unwrap() {
        println!("{}", app);
    }
    println!("**************/");
}

pub async fn open_file(name: &str, flags: OpenFlags) -> Result<Arc<Fat32Inode>, SysError> {
    stack_trace!();
    let _sie = AutoSie::new();
    let (f_r, f_w) = flags.read_write()?;
    if flags.create() {
        todo!()
    }
    let mut stack = Vec::new();
    for s in name
        .split(['/', '\\'])
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        match s {
            "." => continue,
            ".." => {
                stack.pop();
            }
            s => {
                stack.push(s);
            }
        }
    }
    let inode = manager().search_any(&stack).await?;
    let attr = inode.attr();
    let writable = attr.writable();
    if !writable && f_w {
        return Err(SysError::EACCES);
    }
    Ok(Arc::new(Fat32Inode {
        readable: AtomicBool::new(f_r),
        writable: AtomicBool::new(f_w),
        ptr: AtomicUsize::new(0),
        inode,
    }))
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
