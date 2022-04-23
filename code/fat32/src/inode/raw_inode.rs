use core::ops::ControlFlow;

use alloc::{sync::Arc, vec::Vec};

use crate::{
    block::bcache::Cache,
    fat_list::FatList,
    layout::name::Attr,
    mutex::{rw_sleep_mutex::RwSleepMutex, rw_spin_mutex::RwSpinMutex},
    tools::CID,
    xerror::SysError,
    Fat32Manager,
};

use super::{dir_inode::DirInode, file_inode::FileInode, inode_cache::InodeCache, InodeMark};

/// 每个打开的文件将持有一个RawInode
///
/// Inode可以直接从InodeCache产生
pub struct RawInode {
    pub cache: Arc<InodeCache>,
    pub parent: Arc<InodeCache>,
    last_cache: RwSpinMutex<Option<(usize, (CID, Arc<Cache>))>>, // 最近一次访问的块
    _mark: Arc<InodeMark>,
}

impl RawInode {
    pub fn new(cache: Arc<InodeCache>, parent: Arc<InodeCache>, mark: Arc<InodeMark>) -> Self {
        Self {
            parent,
            cache,
            last_cache: RwSpinMutex::new(None),
            _mark: mark,
        }
    }
    pub fn attr(&self) -> Attr {
        self.cache.inner.shared_lock().attr()
    }
    pub fn into_dir(p: Arc<RwSleepMutex<Self>>) -> DirInode {
        unsafe { debug_assert!(p.unsafe_get().attr().contains(Attr::DIRECTORY)) };
        DirInode::new(p)
    }
    pub fn into_file(p: Arc<RwSleepMutex<Self>>) -> FileInode {
        unsafe { debug_assert!(!p.unsafe_get().attr().contains(Attr::DIRECTORY)) };
        FileInode::new(p)
    }
    /// 此函数将修改缓存
    ///
    /// 如果长度不足, 返回Ok(Err(Fat链表长度)))
    async fn get_nth_block_cid(
        &self,
        fat_list: &FatList,
        n: usize,
    ) -> Result<Result<CID, usize>, SysError> {
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
                    return ControlFlow::Break(Ok(prev));
                }
                let this = (cur, cid);
                if cur == n {
                    return ControlFlow::Break(Ok(this));
                }
                return ControlFlow::Continue(this);
            })
            .await;
        save_list
            .into_iter()
            .fold(self.cache.inner.unique_lock(), |mut cache, (cid, cur)| {
                cache.update_list(cid, cur);
                cache
            });
        match r {
            ControlFlow::Break(Err(e)) => Err(e),
            ControlFlow::Continue((off, cid)) | ControlFlow::Break(Ok((off, cid))) => {
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
    async fn get_list_last(&self, fat_list: &FatList) -> Result<Option<(usize, CID)>, SysError> {
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
                    return ControlFlow::Break(Ok(prev));
                }
                ControlFlow::Continue((cur, cid))
            })
            .await;
        save_list
            .into_iter()
            .fold(self.cache.inner.unique_lock(), |mut cache, (cid, cur)| {
                cache.update_list(cid, cur);
                cache
            });
        match r {
            ControlFlow::Break(Err(e)) => Err(e),
            ControlFlow::Continue(tup) | ControlFlow::Break(Ok(tup)) => Ok(Some(tup)),
        }
    }
    /// 如果n超出了fat链表, 返回Ok(Err(链表长度))
    pub async fn get_nth_block(
        &self,
        manager: &Fat32Manager,
        n: usize,
    ) -> Result<Result<(CID, Arc<Cache>), usize>, SysError> {
        if let Some((ln, c)) = &*self.last_cache.shared_lock() {
            if *ln == n {
                return Ok(Ok(c.clone()));
            }
        }
        let ci = &self.cache.inner;
        // 如果和下面写在同一行, 锁shared_lock将会在unique_lock后析构, 导致死锁
        let x = ci.shared_lock().try_get_nth_block_cid(n);
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
                return Ok(Ok((cid, cache)));
            }
            Err(tup) => return Ok(Err(tup)),
        }
    }
    /// 找不到块就分配新的并使用init函数初始化
    pub async fn get_nth_block_alloc<T: Copy>(
        &mut self,
        manager: &Fat32Manager,
        n: usize,
        mut init: impl FnMut(&mut [T]),
    ) -> Result<(CID, Arc<Cache>), SysError> {
        let mut cur_len = match self.get_nth_block(manager, n).await? {
            Ok(tup) => return Ok(tup),
            Err(cur_len) => cur_len,
        };
        let mut cid = match cur_len {
            0 => CID::free(),
            n => self.get_nth_block_cid(&manager.list, n - 1).await?.unwrap(),
        };
        let mut cache = None;
        if cur_len == 0 {
            let new_cid = manager.list.alloc_block().await?;
            cache = Some(manager.caches.get_block_init(new_cid, &mut init).await?);
            self.cache.inner.unique_lock().append_first(new_cid);
            cur_len += 1;
            cid = new_cid;
        }
        while cur_len < n {
            cid = manager.list.alloc_block_after(cid).await?;
            cache = Some(manager.caches.get_block_init(cid, &mut init).await?);
            cur_len += 1;
            self.cache.inner.unique_lock().append_last(cur_len, cid);
        }
        self.cache.update_aid();
        let cache = (cid, cache.unwrap());
        self.last_cache.get_mut().replace((n, cache.clone()));
        Ok(cache)
    }
    pub async fn append_block<T: Copy>(
        &mut self,
        manager: &Fat32Manager,
        mut init: impl FnMut(&mut [T]),
    ) -> Result<(CID, Arc<Cache>), SysError> {
        // if self.cache.inner.shared_lock().l
        todo!()
    }
}
