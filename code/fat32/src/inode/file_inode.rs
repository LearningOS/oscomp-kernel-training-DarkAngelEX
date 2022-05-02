use alloc::sync::Arc;
use ftl_util::error::SysError;

use crate::{layout::name::Attr, mutex::RwSleepMutex, Fat32Manager};

use super::raw_inode::RawInode;

/// 不要在这里维护任何数据 数据都放在inode中
#[derive(Clone)]
pub struct FileInode {
    inode: Arc<RwSleepMutex<RawInode>>,
}

impl FileInode {
    pub(crate) fn new(inode: Arc<RwSleepMutex<RawInode>>) -> Self {
        Self { inode }
    }
    pub fn attr(&self) -> Attr {
        unsafe { self.inode.unsafe_get().attr() }
    }
    /// offset为字节偏移
    pub async fn read_at(
        &self,
        manager: &Fat32Manager,
        offset: usize,
        buffer: &mut [u8],
    ) -> Result<usize, SysError> {
        stack_trace!();
        let inode = &*self.inode.shared_lock().await;
        let bytes = inode.cache.inner.shared_lock().file_bytes();
        let prev_len = buffer.len();
        let end_offset = bytes.min(offset + prev_len);
        let mut buffer = &mut buffer[..prev_len.min(bytes - offset)];
        let mut cur = offset;
        stack_trace!();
        while cur < end_offset {
            let (nth, off) = manager.bpb.cluster_spilt(cur);
            let cache = match inode.get_nth_block(manager, nth).await? {
                Ok((_cid, cache)) => cache,
                Err(_) => return Ok(cur - offset),
            };
            stack_trace!();
            let n = cache
                .access_ro(|s: &[u8]| {
                    let n = buffer.len().min(s.len() - off);
                    buffer[..n].copy_from_slice(&s[off..off + n]);
                    n
                })
                .await;
            cur += n;
            buffer = &mut buffer[n..];
        }
        stack_trace!();
        inode.update_access_time(&manager.utc_time());
        inode.short_entry_sync(manager).await?;
        Ok(cur - offset)
    }
    /// 自动扩容
    pub async fn write_at(
        &self,
        manager: &Fat32Manager,
        offset: usize,
        buffer: &[u8],
    ) -> Result<usize, SysError> {
        let mut cur = offset;
        let inode = self.inode.shared_lock().await;
        let bytes = inode.cache.inner.shared_lock().file_bytes();
        let end_offset = bytes.min(offset + buffer.len());
        let mut buffer_0 = &buffer[..buffer.len().min(bytes.saturating_sub(offset))];
        while cur < end_offset {
            let (nth, off) = manager.bpb.cluster_spilt(cur);
            let (cid, cache) = match inode.get_nth_block(manager, nth).await? {
                Ok(tup) => tup,
                Err(_) => return Ok(cur - offset),
            };
            let n = manager
                .caches
                .write_block(cid, &cache, |s: &mut [u8]| {
                    let n = buffer_0.len().min(s.len() - off);
                    s[off..off + n].copy_from_slice(&buffer_0[..n]);
                    n
                })
                .await?;
            cur += n;
            buffer_0 = &buffer_0[n..];
        }
        debug_assert!(cur <= bytes);
        if cur == bytes {
            inode.update_access_modify_time(&manager.utc_time());
            inode.short_entry_sync(manager).await?;
            return Ok(buffer.len());
        }
        drop(inode); // release shared_lock
        let inode = &mut *self.inode.unique_lock().await;
        let mut buffer = &buffer[cur - offset..];
        while !buffer.is_empty() {
            let (nth, off) = manager.bpb.cluster_spilt(cur);
            let (cid, cache) = inode
                .get_nth_block_alloc(manager, nth, |_: &mut [u8]| ())
                .await?;
            let n = manager
                .caches
                .write_block(cid, &cache, |s: &mut [u8]| {
                    let n = buffer.len().min(s.len() - off);
                    s[off..off + n].copy_from_slice(&buffer[..n]);
                    n
                })
                .await?;
            cur += n;
            buffer = &buffer[n..];
        }
        inode.update_file_bytes(cur);
        inode.update_access_modify_time(&manager.utc_time());
        inode.short_entry_sync(manager).await?;
        Ok(cur - offset)
    }
    /// 在文件末尾写
    pub async fn write_append(
        &self,
        manager: &Fat32Manager,
        mut buffer: &[u8],
    ) -> Result<usize, SysError> {
        let inode = &mut *self.inode.unique_lock().await;
        let offset = inode.cache.inner.shared_lock().file_bytes();
        let mut cur = offset;
        while !buffer.is_empty() {
            let (nth, off) = manager.bpb.cluster_spilt(cur);
            let (cid, cache) = inode
                .get_nth_block_alloc(manager, nth, |_: &mut [u8]| ())
                .await?;
            let n = manager
                .caches
                .write_block(cid, &cache, |s: &mut [u8]| {
                    let n = buffer.len().min(s.len() - off);
                    s[off..off + n].copy_from_slice(&buffer[..n]);
                    n
                })
                .await?;
            cur += n;
            buffer = &buffer[n..];
        }
        inode.update_file_bytes(cur);
        inode.update_access_modify_time(&manager.utc_time());
        inode.short_entry_sync(manager).await?;
        Ok(cur - offset)
    }
}
