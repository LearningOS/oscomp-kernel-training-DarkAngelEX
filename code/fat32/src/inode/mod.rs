use alloc::sync::Arc;
use ftl_util::error::SysError;

use crate::{
    layout::name::{Attr, RawShortName},
    mutex::RwSleepMutex,
    tools::{Align8, CID},
    DirInode, Fat32Manager, FileInode,
};

use self::raw_inode::RawInode;

pub mod dir_inode;
pub mod file_inode;
pub mod inode_cache;
pub mod manager;
pub mod raw_inode;
mod xstr;

#[derive(Clone)]
pub enum AnyInode {
    Dir(DirInode),
    File(FileInode),
}

impl AnyInode {
    pub fn attr(&self) -> Attr {
        match self {
            AnyInode::Dir(v) => v.attr(),
            AnyInode::File(v) => v.attr(),
        }
    }
    pub fn file(&self) -> Option<&FileInode> {
        match self {
            AnyInode::Dir(_) => None,
            AnyInode::File(v) => Some(v),
        }
    }
    pub fn dir(&self) -> Option<&DirInode> {
        match self {
            AnyInode::Dir(v) => Some(v),
            AnyInode::File(_) => None,
        }
    }
    pub fn short_name(&self) -> Align8<RawShortName> {
        unsafe {
            self.raw_inode()
                .unsafe_get()
                .cache
                .inner
                .shared_lock()
                .short
        }
    }
    /// return None of this is dir
    pub fn file_bytes(&self) -> Option<usize> {
        self.file().map(|f| f.bytes())
    }
    pub async fn blk_num(&self, manager: &Fat32Manager) -> Result<usize, SysError> {
        self.raw_inode()
            .shared_lock()
            .await
            .blk_num(&manager.list)
            .await
    }
    fn raw_inode(&self) -> &Arc<RwSleepMutex<RawInode>> {
        match self {
            AnyInode::Dir(v) => &v.inode,
            AnyInode::File(v) => &v.inode,
        }
    }
}

/// 此Inode在manager与所有文件中共享, 强引用计数-1即为打开的文件数量
///
/// 用来动态释放多余的缓存块
pub(crate) struct InodeMark;
/// Inode ID, offset of entry offset
///
/// IID of root is 0
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct IID(u64);

impl IID {
    pub const ROOT: Self = IID(0);
    pub fn is_root(self) -> bool {
        self == Self::ROOT
    }
    /// 文件所在目录簇号 目录项偏移量
    #[inline(always)]
    pub fn new(cid: CID, off: usize, cluster_bytes_log2: u32) -> Self {
        debug_assert!(off < 1 << (cluster_bytes_log2 - 5));
        Self(((cid.0 as u64) << (cluster_bytes_log2 - 5)) | off as u64)
    }
    /// 簇号
    pub fn cid(self, cluster_bytes_log2: u32) -> CID {
        CID((self.0 >> (cluster_bytes_log2 - 5)) as u32)
    }
    /// 簇内目录项偏移
    pub fn off(self, cluster_bytes_log2: u32) -> usize {
        (self.0 as usize) & ((1 << (cluster_bytes_log2 - 5)) - 1)
    }
}
