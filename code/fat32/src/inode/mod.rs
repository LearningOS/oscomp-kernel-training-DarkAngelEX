use crate::{tools::CID, DirInode, FileInode};

pub mod dir_inode;
pub mod file_inode;
pub mod inode_cache;
pub mod manager;
pub mod raw_inode;
mod xstr;

pub enum AnyInode {
    Dir(DirInode),
    File(FileInode),
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
