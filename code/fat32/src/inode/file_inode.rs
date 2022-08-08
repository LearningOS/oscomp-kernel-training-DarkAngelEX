use alloc::sync::Arc;
use ftl_util::error::SysRet;

use crate::{layout::name::Attr, mutex::RwSleepMutex, Fat32Manager};

use super::raw_inode::RawInode;

/// 不要在这里维护任何数据 数据都放在inode中
#[derive(Clone)]
pub struct FileInode {
    pub(crate) inode: Arc<RwSleepMutex<RawInode>>,
}

impl FileInode {
    pub(crate) fn new(inode: Arc<RwSleepMutex<RawInode>>) -> Self {
        Self { inode }
    }
    pub fn attr(&self) -> Attr {
        unsafe { self.inode.unsafe_get().attr() }
    }
    pub fn bytes(&self) -> usize {
        unsafe {
            self.inode
                .unsafe_get()
                .cache
                .inner
                .shared_lock()
                .file_bytes()
        }
    }
    /// 这个函数会让此文件从目录树中移除, 并自己管理数据资源, 在析构时归还资源
    pub async fn detach(&self, manager: &Arc<Fat32Manager>) {
        self.inode.unique_lock().await.detach(manager);
    }
    /// offset为字节偏移
    pub async fn read_at(
        &self,
        manager: &Fat32Manager,
        offset: usize,
        buffer: &mut [u8],
    ) -> SysRet {
        stack_trace!();
        let inode = &*self.inode.shared_lock().await;
        let bytes = inode.cache.inner.shared_lock().file_bytes();
        let prev_len = buffer.len();
        let end_offset = bytes.min(offset + prev_len);
        let buffer_end = prev_len.min(bytes - offset);
        let mut buffer = &mut buffer[..buffer_end];
        let mut cur = offset;
        while cur < end_offset {
            let (nth, off) = manager.bpb.cluster_spilt(cur);
            let cache = match inode.get_nth_block(manager, nth).await? {
                Ok((_cid, cache)) => cache,
                Err(_) => return Ok(cur - offset),
            };
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
        inode.update_access_time(manager.now());
        inode.short_entry_sync(manager).await?;
        Ok(cur - offset)
    }
    /// 自动扩容
    pub async fn write_at(&self, manager: &Fat32Manager, offset: usize, buffer: &[u8]) -> SysRet {
        stack_trace!();
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
        debug_assert!(cur <= end_offset);
        if cur == offset + buffer.len() {
            inode.update_access_modify_time(manager.now());
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
        inode.update_access_modify_time(manager.now());
        inode.short_entry_sync(manager).await?;
        Ok(cur - offset)
    }
    /// 在文件末尾写
    pub async fn write_append(&self, manager: &Fat32Manager, mut buffer: &[u8]) -> SysRet {
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
        inode.update_access_modify_time(manager.now());
        inode.short_entry_sync(manager).await?;
        Ok(cur - offset)
    }
}
