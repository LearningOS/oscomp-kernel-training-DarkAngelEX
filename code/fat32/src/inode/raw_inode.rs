use core::{ops::ControlFlow, ptr::NonNull};

use alloc::{boxed::Box, sync::Arc, vec::Vec};
use ftl_util::{
    async_tools::SendWraper,
    error::{SysError, SysR, SysRet},
    time::{Instant, UtcTime},
};

use crate::{
    block::bcache::Cache,
    fat_list::FatList,
    layout::name::{Attr, RawName},
    mutex::{RwSleepMutex, RwSpinMutex},
    tools::CID,
    Fat32Manager,
};

use super::{dir_inode::DirInode, file_inode::FileInode, inode_cache::InodeCache, InodeMark};

type LastCache = Option<(usize, (CID, Arc<Cache>))>;
/// 每个打开的文件将持有一个RawInode
///
/// Inode可以直接从InodeCache产生
pub(crate) struct RawInode {
    pub cache: Arc<InodeCache>,
    pub parent: Option<Arc<InodeCache>>,
    last_cache: RwSpinMutex<LastCache>, // 最近一次访问的块
    is_root: bool,
    _mark: Arc<InodeMark>,
    manager: Option<SendWraper<NonNull<Fat32Manager>>>, // 只有文件detach以后才存在
}

unsafe impl Send for RawInode {}
unsafe impl Sync for RawInode {}

impl Drop for RawInode {
    fn drop(&mut self) {
        if !self.cache.detached || self.manager.is_none() {
            return;
        }
        debug_assert!(Arc::strong_count(&self.cache) == 1);
        let cid = self.cache.inner.shared_lock().cid_start;
        if !cid.is_free() {
            debug_assert!(cid.is_next());
            let manager = self.manager.take().unwrap();
            let spawner = unsafe { manager.0.as_ref().get_spawner() };
            // 使用'static发送到另一个线程
            spawner.spawn(Box::pin(async move {
                let manager = manager.map(|a| unsafe { &*a.as_ptr() });
                manager.list.free_cluster_at(cid).await.1.unwrap();
                manager.list.free_cluster(cid).await.unwrap();
            }))
        }
    }
}

impl RawInode {
    pub fn new(
        cache: Arc<InodeCache>,
        parent: Arc<InodeCache>,
        mark: Arc<InodeMark>,
        is_root: bool,
    ) -> Self {
        Self {
            cache,
            parent: Some(parent),
            last_cache: RwSpinMutex::new(None),
            is_root,
            _mark: mark,
            manager: None,
        }
    }
    /// 只有目录文件才可以调用此函数! 目录文件保证父目录存在
    ///
    /// 普通文件需要由VFS代理获取目录项
    pub fn parent(&self) -> &InodeCache {
        self.parent.as_ref().unwrap()
    }
    pub fn attr(&self) -> Attr {
        unsafe { self.cache.inner.unsafe_get().attr() }
    }
    pub async fn blk_num(&self, fat_list: &FatList) -> SysRet {
        let n = match self.get_list_last(fat_list).await? {
            Some((n, _)) => n + 1,
            None => 0,
        };
        Ok(n)
    }
    pub fn into_dir(p: Arc<RwSleepMutex<Self>>) -> DirInode {
        unsafe { debug_assert!(p.unsafe_get().attr().contains(Attr::DIRECTORY)) };
        DirInode::new(p)
    }
    pub fn into_file(p: Arc<RwSleepMutex<Self>>) -> FileInode {
        unsafe { debug_assert!(!p.unsafe_get().attr().contains(Attr::DIRECTORY)) };
        FileInode::new(p)
    }
    pub fn update_file_bytes(&self, bytes: usize) {
        self.cache.inner.unique_lock().update_file_bytes(bytes);
    }
    pub fn update_access_time(&self, now: Instant) {
        let utc = &UtcTime::from_instant(now);
        self.cache.inner.unique_lock().update_access_time(utc);
    }
    pub fn update_modify_time(&self, now: Instant) {
        let utc = &UtcTime::from_instant(now);
        self.cache.inner.unique_lock().update_modify_time(utc);
    }
    pub fn update_access_modify_time(&self, now: Instant) {
        let utc = &UtcTime::from_instant(now);
        let lock = &mut *self.cache.inner.unique_lock();
        lock.update_access_time(utc);
        lock.update_modify_time(utc);
    }
    pub fn available(&self) -> SysR<()> {
        if self.cache.detached {
            Err(SysError::ENOENT)
        } else {
            Ok(())
        }
    }
    /// 将此文件inode从目录中移除
    pub fn detach_file(&mut self, manager: &Fat32Manager) -> SysR<()> {
        debug_assert!(self.parent.is_some()); // 禁止detach两次
        self.parent = None;
        self.cache = self.cache.detach_file();
        self.manager = unsafe {
            Some(SendWraper::new(
                NonNull::new(manager as *const _ as *mut _).unwrap(),
            ))
        };
        Ok(())
    }
    /// 将此目录inode从目录中移除
    pub fn detach_dir(&mut self) {
        debug_assert!(self.parent.is_some()); // 禁止detach两次
        self.parent = None;
        self.cache = self.cache.detach_dir();
    }
    /// 此函数将更新缓存
    ///
    /// 如果长度不足, 返回Ok(Err(Fat链表长度)))
    async fn get_nth_block_cid(&self, fat_list: &FatList, n: usize) -> SysR<Result<CID, usize>> {
        stack_trace!();
        let (cur, cid) = {
            let cache = self.cache.inner.shared_lock();
            if let Some(x) = cache.try_get_nth_block_cid(n) {
                return Ok(x);
            }
            cache.list_last_save().unwrap()
        };
        let mut save_list = Vec::new();
        let r = fat_list
            .travel(cid, cur, (cur, cid), |prev, cid, cur| {
                save_list.push((cid, cur));
                if !cid.is_next() {
                    return ControlFlow::Break(prev);
                }
                let this = (cur, cid);
                if cur == n {
                    return ControlFlow::Break(this);
                }
                try { this }
            })
            .await?;
        let mut lock = self.cache.inner.unique_lock();
        save_list.into_iter().for_each(move |(cid, cur)| {
            lock.update_list(cid, cur);
        });
        match r {
            ControlFlow::Continue((off, cid)) | ControlFlow::Break((off, cid)) => {
                if off < n {
                    Ok(Err(off + 1))
                } else {
                    Ok(Ok(cid))
                }
            }
        }
    }
    /// 返回最后一个簇的(偏移, CID) 链表长度为偏移+1
    ///
    /// 空链表返回None
    async fn get_list_last(&self, fat_list: &FatList) -> SysR<Option<(usize, CID)>> {
        let last_save = self.cache.inner.shared_lock().list_last_save();
        let (cur, cid) = match last_save {
            None => return Ok(None),
            Some(tup) => tup,
        };
        let mut save_list = Vec::new();
        let r = fat_list
            .travel(cid, cur, (cur, cid), |prev, cid, cur| {
                save_list.push((cid, cur));
                if !cid.is_next() {
                    return ControlFlow::Break(prev);
                }
                ControlFlow::Continue((cur, cid))
            })
            .await?;
        let mut lock = self.cache.inner.unique_lock();
        save_list.into_iter().for_each(move |(cid, cur)| {
            lock.update_list(cid, cur);
        });
        match r {
            ControlFlow::Continue(tup) | ControlFlow::Break(tup) => Ok(Some(tup)),
        }
    }
    /// 获取第n个簇(首个簇为0)
    ///
    /// 如果n超出了fat链表, 返回Ok(Err(链表长度))
    pub fn get_nth_block_fast(
        &self,
        manager: &Fat32Manager,
        n: usize,
    ) -> SysR<Result<(CID, Arc<Cache>), usize>> {
        if let Some((ln, c)) = &*self.last_cache.shared_lock() {
            if *ln == n {
                return Ok(Ok(c.clone()));
            }
        }
        let x = self.cache.inner.shared_lock().try_get_nth_block_cid(n);
        let x = match x {
            Some(x) => x,
            None => return Err(SysError::EAGAIN),
        };
        match x {
            Ok(cid) => {
                let cache = manager.caches.get_block_fast(cid)?;
                self.last_cache
                    .unique_lock()
                    .replace((n, (cid, cache.clone())));
                Ok(Ok((cid, cache)))
            }
            Err(tup) => Ok(Err(tup)),
        }
    }
    /// 获取第n个簇(首个簇为0)
    ///
    /// 如果n超出了fat链表, 返回Ok(Err(链表长度))
    pub async fn get_nth_block(
        &self,
        manager: &Fat32Manager,
        n: usize,
    ) -> SysR<Result<(CID, Arc<Cache>), usize>> {
        stack_trace!();
        if let Some((ln, c)) = &*self.last_cache.shared_lock() {
            if *ln == n {
                return Ok(Ok(c.clone()));
            }
        }
        // 如果和下面写在同一行, 锁shared_lock将会在unique_lock后析构, 导致死锁
        let x = self.cache.inner.shared_lock().try_get_nth_block_cid(n);
        let x = if let Some(x) = x {
            x
        } else {
            self.get_nth_block_cid(&manager.list, n).await?
        };
        match x {
            Ok(cid) => {
                let cache = manager.caches.get_block(cid).await?;
                self.last_cache
                    .unique_lock()
                    .replace((n, (cid, cache.clone())));
                Ok(Ok((cid, cache)))
            }
            Err(tup) => Ok(Err(tup)),
        }
    }
    /// 找不到块就分配新的并使用init函数初始化
    pub async fn get_nth_block_alloc<T: Copy>(
        &mut self,
        manager: &Fat32Manager,
        n: usize,
        mut init: impl FnMut(&mut [T]),
    ) -> SysR<(CID, Arc<Cache>)> {
        let mut cur_len = match self.get_nth_block(manager, n).await? {
            Ok(tup) => return Ok(tup),
            Err(cur_len) => cur_len,
        };
        let mut cid = match cur_len {
            0 => CID::FREE,
            n => self.get_nth_block_cid(&manager.list, n - 1).await?.unwrap(),
        };
        let mut cache = None;
        if cur_len == 0 {
            let new_cid = manager.list.alloc_block().await?;
            cache = Some(manager.caches.get_block_init(new_cid, &mut init).await?);
            self.cache.inner.unique_lock().append_first(new_cid);
            cur_len += 1;
            cid = new_cid;
        } else if cur_len == n {
            cache = Some(manager.caches.get_block(cid).await?);
        }
        let mut update = Vec::new();
        while cur_len < n {
            cid = manager.list.alloc_block_after(cid).await?;
            cache = Some(manager.caches.get_block_init(cid, &mut init).await?);
            cur_len += 1;
            update.push((cur_len, cid));
        }
        let mut lock = self.cache.inner.unique_lock();
        update
            .into_iter()
            .for_each(move |(n, cid)| lock.append_last(n, cid));
        self.cache.update_aid();
        let cache = (cid, cache.unwrap());
        self.last_cache.get_mut().replace((n, cache.clone()));
        Ok(cache)
    }
    pub async fn append_block<T: Copy>(
        &mut self,
        manager: &Fat32Manager,
        init: impl FnMut(&mut [T]),
    ) -> SysR<(usize, CID, Arc<Cache>)> {
        let (n, cid) = match self.get_list_last(&manager.list).await? {
            None => (0, manager.list.alloc_block().await?),
            Some((off, cid)) => (off, manager.list.alloc_block_after(cid).await?),
        };
        let cache = manager.caches.get_block_init(cid, init).await?;
        self.cache.inner.unique_lock().append_last(n, cid);
        Ok((n, cid, cache))
    }
    /// 重置链表长度
    pub async fn resize<T: Copy>(
        &mut self,
        manager: &Fat32Manager,
        n: usize,
        init: impl FnMut(&mut [T]),
    ) -> SysR<()> {
        if n == 0 {
            let cid = match self.get_nth_block_cid(&manager.list, 0).await? {
                Err(len) => {
                    debug_assert_eq!(len, 0);
                    return Ok(());
                }
                Ok(cid) => cid,
            };
            manager.list.free_cluster_at(cid).await.1?;
            manager.list.free_cluster(cid).await?;
            self.cache.inner.unique_lock().list_truncate(0, CID::FREE);
        } else {
            match self.get_nth_block_cid(&manager.list, n - 1).await? {
                Err(_) => {
                    self.get_nth_block_alloc(manager, n - 1, init).await?;
                }
                Ok(cid) => {
                    manager.list.free_cluster_at(cid).await.1?;
                    self.cache.inner.unique_lock().list_truncate(n, cid);
                }
            }
        }
        Ok(())
    }
    pub fn short_entry_sync_fast(&self, manager: &Fat32Manager) -> SysR<()> {
        stack_trace!();
        if self.is_root {
            return Ok(());
        }
        let (entry, short) = self.cache.inner.shared_lock().entry();
        // 这个文件已经detach
        if entry.cid == CID::FREE {
            return Ok(());
        }
        let cache = manager.caches.get_block_fast(entry.cid)?;
        manager
            .caches
            .wirte_block_fast(entry.cid, &cache, |a: &mut [RawName]| {
                a[entry.entry_off].set_short(&short);
            })?;
        Ok(())
    }
    pub async fn short_entry_sync(&self, manager: &Fat32Manager) -> SysR<()> {
        stack_trace!();
        if self.is_root {
            return Ok(());
        }
        let (entry, short) = self.cache.inner.shared_lock().entry();
        // 这个文件已经detach
        if entry.cid == CID::FREE {
            return Ok(());
        }
        let cache = manager.caches.get_block(entry.cid).await?;
        manager
            .caches
            .write_block(entry.cid, &cache, |a: &mut [RawName]| {
                a[entry.entry_off].set_short(&short);
            })
            .await?;
        Ok(())
    }
}
