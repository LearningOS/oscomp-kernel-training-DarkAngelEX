use alloc::sync::Arc;

use crate::{mutex::rw_sleep_mutex::RwSleepMutex, xerror::SysError, Fat32Manager};

use super::raw_inode::RawInode;

/// 不要在这里维护任何数据 数据都放在inode中
pub struct FileInode {
    inode: Arc<RwSleepMutex<RawInode>>,
}

impl FileInode {
    pub fn new(inode: Arc<RwSleepMutex<RawInode>>) -> Self {
        Self { inode }
    }
    /// offset为字节偏移 Ok(Err)则为超出了范围
    pub async fn read_at<T: Copy>(
        &self,
        manager: &Fat32Manager,
        offset: usize,
        buffer: &mut [u8],
    ) -> Result<(), SysError> {
        let (c, o) = manager.bpb.cluster_spilt(offset);
        todo!()
    }
    // 如果返回 Ok(Err)则为超出了范围, 什么也不会做
    pub async fn write_at_no_resize<T: Copy>(
        &self,
        manager: &Fat32Manager,
        offset: usize,
        buffer: &[u8],
    ) -> Result<Result<(), ()>, SysError> {
        todo!()
    }
    // 自动扩容
    pub async fn write_at<T: Copy>(
        &mut self,
        manager: &Fat32Manager,
        offset: usize,
        buffer: &[u8],
    ) -> Result<(), SysError> {
        todo!()
    }
    // 在文件末尾写
    pub async fn write_append<T: Copy>(
        &mut self,
        manager: &Fat32Manager,
        buffer: &[u8],
    ) -> Result<(), SysError> {
        todo!()
    }
}
