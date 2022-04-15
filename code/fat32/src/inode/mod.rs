use alloc::boxed::Box;

use crate::tools::CID;

pub mod inode_cache;
pub mod manager;

/// Inode ID
pub struct IID(u32);

/// 每个打开的文件将持有一个Inode
pub struct Fat32Inode {
    cid: CID,
    clear: Option<Box<dyn FnOnce(CID)>>,
}

impl Drop for Fat32Inode {
    fn drop(&mut self) {
        self.clear.take().map(|f| f(self.cid));
    }
}

impl Fat32Inode {
    pub fn new(cid: CID) -> Self {
        Self { cid, clear: None }
    }
    pub fn set_clear_fn(&mut self, clear: Box<dyn FnOnce(CID)>) {
        self.clear = Some(clear)
    }
}
