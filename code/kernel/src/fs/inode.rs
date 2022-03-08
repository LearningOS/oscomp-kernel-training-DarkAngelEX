use super::{AsyncFileOutput, File};
use crate::{
    drivers::BLOCK_DEVICE,
    memory::allocator::frame,
    process::Process,
    sync::mutex::SpinNoIrqLock,
    syscall::SysError,
    user::{UserData, UserDataMut},
};
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::lazy::OnceCell;
use easy_fs::{EasyFileSystem, Inode};

pub struct OSInode {
    readable: bool,
    writable: bool,
    inner: SpinNoIrqLock<OSInodeInner>,
}

pub struct OSInodeInner {
    offset: usize,
    inode: Arc<Inode>,
}

impl OSInode {
    pub fn new(readable: bool, writable: bool, inode: Arc<Inode>) -> Self {
        Self {
            readable,
            writable,
            inner: SpinNoIrqLock::new(OSInodeInner { offset: 0, inode }),
        }
    }
    pub async fn read_all(&self) -> Vec<u8> {
        let mut inner = self.inner.lock(place!());
        let mut buffer = [0u8; 512];
        let mut v: Vec<u8> = Vec::new();
        loop {
            let len = inner.inode.read_at(inner.offset, &mut buffer);
            if len == 0 {
                break;
            }
            inner.offset += len;
            v.extend_from_slice(&buffer[..len]);
        }
        v
    }
}

static mut ROOT_INODE: OnceCell<Arc<Inode>> = OnceCell::new();

pub fn init() {
    let efs = EasyFileSystem::open(BLOCK_DEVICE.clone());
    unsafe {
        ROOT_INODE
            .set(Arc::new(EasyFileSystem::root_inode(&efs)))
            .unwrap_or_else(|_e| panic!("fs double init"))
    };
}

fn root_inode() -> &'static Arc<Inode> {
    unsafe { ROOT_INODE.get().unwrap() }
}

pub fn list_apps() {
    println!("/**** APPS ****");
    for app in root_inode().ls() {
        println!("{}", app);
    }
    println!("**************/");
}

bitflags! {
    pub struct OpenFlags: u32 {
        const RDONLY = 0;
        const WRONLY = 1 << 0;
        const RDWR = 1 << 1;
        const CREATE = 1 << 9;
        const TRUNC = 1 << 10;
    }
}

impl OpenFlags {
    /// Do not check validity for simplicity
    /// Return (readable, writable)
    pub fn read_write(&self) -> (bool, bool) {
        if self.is_empty() {
            (true, false)
        } else if self.contains(Self::WRONLY) {
            (false, true)
        } else {
            (true, true)
        }
    }
}

pub fn open_file(name: &str, flags: OpenFlags) -> Option<Arc<OSInode>> {
    let (readable, writable) = flags.read_write();
    let root_inode = root_inode();
    if flags.contains(OpenFlags::CREATE) {
        if let Some(inode) = root_inode.find(name) {
            // clear size
            inode.clear();
            Some(Arc::new(OSInode::new(readable, writable, inode)))
        } else {
            // create file
            root_inode
                .create(name)
                .map(|inode| Arc::new(OSInode::new(readable, writable, inode)))
        }
    } else {
        root_inode.find(name).map(|inode| {
            if flags.contains(OpenFlags::TRUNC) {
                inode.clear();
            }
            Arc::new(OSInode::new(readable, writable, inode))
        })
    }
}

impl File for OSInode {
    fn readable(&self) -> bool {
        self.readable
    }
    fn writable(&self) -> bool {
        self.writable
    }
    fn read(self: Arc<Self>, buf: UserDataMut<u8>) -> AsyncFileOutput {
        let mut inner = self.inner.lock(place!());
        let mut total_read_size = 0usize;
        let buffer = match frame::global::alloc() {
            Ok(f) => f,
            Err(_e) => return Box::pin(async { Err(SysError::ENOMEM) }),
        };
        for slice in buf.write_only_iter(buffer) {
            let read_size = inner.inode.read_at(inner.offset, slice);
            if read_size == 0 {
                break;
            }
            inner.offset += read_size;
            total_read_size += read_size;
        }
        Box::pin(async move { Ok(total_read_size) })
    }
    fn write(self: Arc<Self>, buf: UserData<u8>) -> AsyncFileOutput {
        let mut inner = self.inner.lock(place!());
        let mut total_write_size = 0usize;
        let buffer = match frame::global::alloc() {
            Ok(f) => f,
            Err(_e) => return Box::pin(async { Err(SysError::ENOMEM) }),
        };
        for slice in buf.read_only_iter(buffer) {
            let write_size = inner.inode.write_at(inner.offset, slice);
            assert_eq!(write_size, slice.len());
            inner.offset += write_size;
            total_write_size += write_size;
        }
        Box::pin(async move { Ok(total_write_size) })
    }
}
