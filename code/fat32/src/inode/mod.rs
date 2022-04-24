use crate::tools::CID;

pub mod dir_inode;
pub mod file_inode;
pub mod inode_cache;
pub mod manager;
pub mod raw_inode;
mod xstr;

/// 此Inode在manager与所有文件中共享, 强引用计数-1即为打开的文件数量
///
/// 用来动态释放多余的缓存块
pub struct InodeMark;
/// Inode ID, offset of entry offset
///
/// IID of root is 0
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct IID(u64);

impl IID {
    pub fn invalid() -> Self {
        IID(u64::MAX)
    }
    pub fn root() -> Self {
        IID(0)
    }
    pub fn is_root(self) -> bool {
        self == Self::root()
    }
    /// 文件所在目录簇号 目录项偏移量
    pub fn new(cid: CID, off: usize, cluster_bytes_log2: usize) -> Self {
        debug_assert!(off < 1 << (cluster_bytes_log2 as u32) - 5);
        Self(((cid.0 as u64) << ((cluster_bytes_log2 as u32) - 5)) | off as u64)
    }
    /// 簇号
    pub fn cid(self, cluster_bytes_log2: usize) -> CID {
        CID((self.0 >> ((cluster_bytes_log2 as u32) - 5)) as u32)
    }
    /// 簇内目录项偏移
    pub fn off(self, cluster_bytes_log2: usize) -> usize {
        (self.0 as usize) & ((1 << (cluster_bytes_log2 as u32) - 5) - 1)
    }
}
