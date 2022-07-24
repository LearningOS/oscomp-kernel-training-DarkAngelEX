use alloc::sync::Arc;
use ftl_util::{
    error::{SysError, SysR, SysRet},
    time::Instant,
};

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
    pub fn file(&self) -> SysR<&FileInode> {
        match self {
            AnyInode::Dir(_) => Err(SysError::EISDIR),
            AnyInode::File(v) => Ok(v),
        }
    }
    pub fn dir(&self) -> SysR<&DirInode> {
        match self {
            AnyInode::Dir(v) => Ok(v),
            AnyInode::File(_) => Err(SysError::ENOTDIR),
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
    pub fn file_bytes(&self) -> SysR<usize> {
        self.file().map(|f| f.bytes())
    }
    pub async fn blk_num(&self, manager: &Fat32Manager) -> SysRet {
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
    pub async fn update_time(&self, access: Option<Instant>, modify: Option<Instant>) {
        if access.is_none() && modify.is_none() {
            return;
        }
        let lk = self.raw_inode().unique_lock().await;
        if let Some(ut) = access {
            lk.update_access_time(ut)
        }
        if let Some(ut) = modify {
            lk.update_modify_time(ut)
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
    pub fn get(self) -> u64 {
        self.0
    }
}
