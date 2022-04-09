use core::ops::Deref;

use alloc::{boxed::Box, sync::Arc};

/// 用于无阻塞写IO
///
/// 当Shared引用计数为1时尝试写可以无复制地转换为Unique
///
/// 引用计数不为1时会分配内存并复制
pub enum Buffer {
    Unique(Box<[u8]>),
    Shared(SharedBuffer),
}
/// 使用两层指针的原因维持内存分配状态
///
/// 由于长度对齐到2的幂次 Arc<[u8]>在分配内存时会在前增加引用计数成员, 这会导致伙伴分配器浪费一倍的内存!
#[derive(Clone)]
pub struct SharedBuffer(pub Arc<Box<[u8]>>);

impl Deref for SharedBuffer {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

impl Buffer {
    pub fn new(size: usize) -> Result<Self, ()> {
        unsafe {
            let ptr = Box::try_new_uninit_slice(size)
                .map_err(|_| ())?
                .assume_init();
            Ok(Self::Unique(ptr))
        }
    }
    pub fn share(&mut self) -> SharedBuffer {
        let ptr = match self {
            Buffer::Shared(ptr) => return ptr.clone(),
            Buffer::Unique(ptr) => ptr,
        };
        unsafe {
            let ptr = core::ptr::read(ptr);
            let ptr = SharedBuffer(Arc::new(ptr));
            let ret = ptr.clone();
            core::ptr::write(self, Buffer::Shared(ptr));
            ret
        }
    }
    pub fn access_ro(&self) -> &[u8] {
        match self {
            Buffer::Unique(ptr) => ptr,
            Buffer::Shared(ptr) => ptr,
        }
    }
    pub fn access_rw(&mut self) -> Result<&mut [u8], ()> {
        match self {
            Buffer::Unique(ptr) => Ok(ptr),
            Buffer::Shared(SharedBuffer(ptr)) => {
                if Arc::strong_count(ptr) == 1 {
                    // 将Shared强转成Unique
                    let ptr = unsafe { core::ptr::read(ptr) };
                    let new: Box<[u8]> = Arc::try_unwrap(ptr).unwrap();
                    unsafe { core::ptr::write(self, Buffer::Unique(new)) };
                } else {
                    // 分配空间
                    let mut new = unsafe {
                        Box::try_new_uninit_slice(ptr.len())
                            .map_err(|_| ())?
                            .assume_init()
                    };
                    new.copy_from_slice(ptr);
                    *self = Buffer::Unique(new);
                }
                match self {
                    Buffer::Unique(ptr) => Ok(ptr),
                    Buffer::Shared(_) => unreachable!(),
                }
            }
        }
    }
}
