use crate::{
    drivers,
    fs::{AsyncFile, File, OpenFlags},
    memory::allocator::frame,
    sync::mutex::SpinNoIrqLock,
    syscall::SysError,
    user::{UserData, UserDataMut},
};
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::lazy::OnceCell;
use easy_fs::{EasyFileSystem, Inode};

pub struct EasyFsInode {
    readable: bool,
    writable: bool,
    inner: SpinNoIrqLock<EasyFsInodeInner>,
}

struct EasyFsInodeInner {
    offset: usize,
    inode: Arc<Inode>,
}

impl EasyFsInode {
    /// parameter: (readable, writable), inode
    pub fn new((readable, writable): (bool, bool), inode: Arc<Inode>) -> Self {
        Self {
            readable,
            writable,
            inner: SpinNoIrqLock::new(EasyFsInodeInner { offset: 0, inode }),
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

struct EasyFsDevice(Arc<dyn drivers::BlockDevice>);

pub fn init(block_device: Arc<dyn easy_fs::BlockDevice>) {
    let efs = EasyFileSystem::open(block_device);
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

pub fn open_file(name: &str, flags: OpenFlags) -> Result<Arc<EasyFsInode>, SysError> {
    let rw = flags.read_write()?;
    let root_inode = root_inode();
    if flags.contains(OpenFlags::CREAT) {
        if let Some(inode) = root_inode.find(name) {
            // clear size
            inode.clear();
            Ok(Arc::new(EasyFsInode::new(rw, inode)))
        } else {
            // create file
            root_inode
                .create(name)
                .map(|inode| Arc::new(EasyFsInode::new(rw, inode)))
                .ok_or(SysError::ENFILE)
        }
    } else {
        root_inode
            .find(name)
            .map(|inode| {
                if flags.contains(OpenFlags::TRUNC) {
                    inode.clear();
                }
                Arc::new(EasyFsInode::new(rw, inode))
            })
            .ok_or(SysError::ENFILE)
    }
}

impl File for EasyFsInode {
    fn readable(&self) -> bool {
        self.readable
    }
    fn writable(&self) -> bool {
        self.writable
    }
    fn read(&self, buf: UserDataMut<u8>) -> AsyncFile {
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
    fn write(&self, buf: UserData<u8>) -> AsyncFile {
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
