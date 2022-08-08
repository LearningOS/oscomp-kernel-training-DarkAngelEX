use core::cell::SyncUnsafeCell;

use alloc::{
    sync::{Arc, Weak},
    vec::Vec,
};
use ftl_util::time::UtcTime;

use crate::{
    layout::name::{Attr, RawShortName},
    mutex::{RwSleepMutex, RwSpinMutex},
    tools::{AIDAllocator, Align8, AID, CID},
    Fat32Manager,
};

use super::{dir_inode::EntryPlace, raw_inode::RawInode, InodeMark};

/// 主要缓存FAT链表 不缓存数据
///
/// 此缓存块的全部操作都是同步操作
pub(crate) struct InodeCache {
    pub inner: RwSpinMutex<InodeCacheInner>,
    aid_alloc: Arc<AIDAllocator>,
    alive: Weak<InodeMark>,
    pub aid: SyncUnsafeCell<AID>,
    pub detached: bool,
}

pub(crate) struct InodeCacheInner {
    pub inode: Weak<RwSleepMutex<RawInode>>, // 自身inode
    pub entry: EntryPlace,                   // 文件目录项地址
    pub cid_list: Vec<CID>,                  // FAT链表缓存 所有CID都是有效的
    pub cid_start: CID,                      // short中的首簇号 空文件是CID::FREE
    pub almost_last: (usize, CID), // 缓存访问到的最后一个有效块和簇偏移 空文件为0 CID::FREE
    pub len: Option<usize>,        // 文件簇数
    pub short: Align8<RawShortName>,
}

impl InodeCacheInner {
    fn new(short: Align8<RawShortName>, entry: EntryPlace) -> Self {
        let cid_start = short.cid();
        let mut cid_list = Vec::new();
        let mut len = None;
        if cid_start.is_next() {
            cid_list.push(cid_start);
        } else {
            len = Some(0);
        }
        InodeCacheInner {
            entry,
            inode: Weak::new(),
            cid_list,
            cid_start,
            almost_last: (0, cid_start),
            len,
            short,
        }
    }
    pub fn entry(&self) -> (EntryPlace, Align8<RawShortName>) {
        (self.entry, self.short)
    }
}

impl InodeCache {
    fn new(
        short: Align8<RawShortName>,
        entry: EntryPlace,
        alive: Weak<InodeMark>,
        aid_alloc: Arc<AIDAllocator>,
    ) -> Self {
        debug_assert!(alive.strong_count() != 0);
        let inner = InodeCacheInner::new(short, entry);
        Self {
            inner: RwSpinMutex::new(inner),
            aid_alloc,
            alive,
            aid: SyncUnsafeCell::new(AID(0)),
            detached: false,
        }
    }
    /// will shared lock inode.cache.inner
    pub fn from_parent(
        manager: &Fat32Manager,
        short: Align8<RawShortName>,
        entry: EntryPlace,
        inode: &RawInode,
    ) -> Self {
        debug_assert!(inode.cache.alive.strong_count() != 0);
        InodeCache::new(
            short,
            entry,
            inode.cache.alive.clone(),
            manager.inodes.aid_alloc.clone(),
        )
    }
    pub fn new_root(manager: &Fat32Manager) -> Self {
        let mut short = Align8(RawShortName::zeroed());
        short.init_dot_dir(1, CID(manager.bpb.root_cluster_id), manager.now());
        let entry = EntryPlace::ROOT;
        let alive = manager.inodes.alive_weak();
        let aid_alloc = manager.inodes.aid_alloc.clone();
        Self::new(short, entry, alive, aid_alloc)
    }
    // inode缓存块替换时使用
    pub fn init(&mut self, short: Align8<RawShortName>, entry: EntryPlace) {
        let inner = self.inner_mut();
        debug_assert!(inner.inode.strong_count() == 0);
        *inner = InodeCacheInner::new(short, entry);
    }
    pub fn inner_mut(&mut self) -> &mut InodeCacheInner {
        self.inner.get_mut()
    }
    /// 尝试快速获取inode
    pub fn try_get_inode(self: &Arc<Self>) -> Option<Arc<RwSleepMutex<RawInode>>> {
        self.inner.try_shared_lock()?.inode.upgrade()
    }
    pub fn get_inode(self: &Arc<Self>, parent: Arc<InodeCache>) -> Arc<RwSleepMutex<RawInode>> {
        let lock = &mut *self.inner.unique_lock();
        if let Some(i) = lock.inode.upgrade() {
            return i;
        }
        let alive = self.alive.upgrade().unwrap();
        let inode = RawInode::new(self.clone(), parent, alive, false);
        let inode = Arc::new(RwSleepMutex::new(inode));
        lock.inode = Arc::downgrade(&inode);
        inode
    }
    pub unsafe fn get_root_inode(self: &Arc<Self>) -> Arc<RwSleepMutex<RawInode>> {
        let lock = &mut *self.inner.unique_lock();
        if let Some(i) = lock.inode.upgrade() {
            return i;
        }
        let alive = self.alive.upgrade().unwrap();
        let inode = RawInode::new(self.clone(), self.clone(), alive, true);
        let inode = Arc::new(RwSleepMutex::new(inode));
        lock.inode = Arc::downgrade(&inode);
        inode
    }
    pub fn aid(&self) -> AID {
        unsafe { *self.aid.get() }
    }
    pub fn update_aid(&self) -> AID {
        let aid = self.aid_alloc.alloc();
        unsafe { *self.aid.get() = aid };
        aid
    }
    /// 从目录树中删除这个文件
    pub fn detach_file(&self) -> Arc<InodeCache> {
        debug_assert!(!self.detached);
        // 生成一个新的cache节点, 原cache的节点引用计数就不存在了
        let mut inner = self.inner.unique_lock();
        let new = Arc::new(InodeCache {
            inner: RwSpinMutex::new(InodeCacheInner {
                inode: inner.inode.clone(),
                entry: EntryPlace::ROOT,
                cid_list: core::mem::take(&mut inner.cid_list),
                cid_start: inner.cid_start,
                almost_last: inner.almost_last,
                len: inner.len,
                short: inner.short,
            }),
            aid_alloc: self.aid_alloc.clone(),
            alive: self.alive.clone(),
            aid: SyncUnsafeCell::new(self.aid()),
            detached: true,
        });
        inner.inode = Weak::new();
        inner.list_truncate(0, CID::FREE);
        inner.entry = EntryPlace::ROOT;
        new
    }
    /// 从目录树中删除这个目录, 这个inode就无效了
    pub fn detach_dir(&self) -> Arc<InodeCache> {
        debug_assert!(!self.detached);
        // 生成一个新的cache节点, 原cache的节点引用计数就不存在了
        let mut inner = self.inner.unique_lock();
        inner.list_truncate(0, CID::FREE);
        inner.entry = EntryPlace::ROOT;
        inner.inode = Weak::new();
        let new = Arc::new(InodeCache {
            inner: RwSpinMutex::new(InodeCacheInner {
                inode: Weak::new(),
                entry: EntryPlace::ROOT,
                cid_list: Vec::new(),
                cid_start: CID::FREE,
                almost_last: (0, CID::FREE),
                len: Some(0),
                short: inner.short,
            }),
            aid_alloc: self.aid_alloc.clone(),
            alive: self.alive.clone(),
            aid: SyncUnsafeCell::new(self.aid()),
            detached: true,
        });
        new
    }
}

impl InodeCacheInner {
    pub fn attr(&self) -> Attr {
        self.short.attributes
    }
    pub fn file_bytes(&self) -> usize {
        debug_assert!(!self.attr().contains(Attr::DIRECTORY));
        self.short.file_bytes()
    }
    pub fn update_file_bytes(&mut self, bytes: usize) {
        self.short.set_file_bytes(bytes);
    }
    pub fn update_modify_time(&mut self, utc_time: &UtcTime) {
        self.short.set_modify_time(utc_time);
    }
    pub fn update_access_time(&mut self, utc_time: &UtcTime) {
        self.short.set_access_time(utc_time);
    }
    pub fn update_list(&mut self, cid: CID, n: usize) {
        if let Some(&x) = self.cid_list.get(n) {
            debug_assert!(x == cid);
            return;
        }
        if !cid.is_next() {
            self.len = Some(n);
            return;
        }
        if self.almost_last.0 <= n {
            self.almost_last = (n, cid);
        }
        if self.cid_list.len() == n {
            self.cid_list.push(cid);
        }
    }
    /// list的长度变为至多n cid为最后一个簇
    pub fn list_truncate(&mut self, n: usize, cid: CID) {
        if n == 0 {
            self.cid_start = CID::FREE;
            self.short.set_cluster(CID::FREE);
            self.cid_list.clear();
            self.almost_last = (0, CID::FREE);
            self.len = Some(0);
            return;
        }
        self.cid_list.truncate(n);
        if self.cid_list.len() * 2 + 100 < self.cid_list.capacity() {
            self.cid_list.shrink_to_fit();
        }
        self.almost_last = (n - 1, cid);
        self.len = Some(n);
    }
    /// 返回缓存的最后一个簇偏移与ID
    ///
    /// 返回None的唯一可能是文件未分配空间
    pub fn list_last_save(&self) -> Option<(usize, CID)> {
        if self.cid_start.is_free() {
            debug_assert!(self.almost_last == (0, CID::FREE));
            return None;
        }
        debug_assert!(self.cid_start.is_next());
        debug_assert!(self.almost_last.1.is_next());
        Some(self.almost_last)
    }
    /// Some(Ok) 找到结果 保证是有效CID
    /// Some(Err) 链表长度不足 返回链表长度
    /// None 缓存不足
    pub fn try_get_nth_block_cid(&self, n: usize) -> Option<Result<CID, usize>> {
        if self.cid_start.is_free() {
            debug_assert!(!self.cid_start.is_next());
            debug_assert!(self.almost_last == (0, CID::FREE));
            return Some(Err(0));
        }
        if let Some(x) = self.len {
            if x <= n {
                return Some(Err(x));
            }
        }
        if let Some(&cid) = self.cid_list.get(n) {
            return Some(Ok(cid));
        }
        let (off, cid) = self.almost_last;
        if off == n {
            return Some(Ok(cid));
        }
        None
    }
    /// 返回缓存的最后一个块
    ///
    /// 空文件返回None
    pub fn get_almost_last_block(&self) -> Option<(usize, CID)> {
        if !self.cid_start.is_free() {
            return None;
        }
        debug_assert!(self.cid_start.is_next());
        let ret = (0, self.cid_start);

        Some(ret)
    }
    /// 返回簇偏移, 簇ID
    /// Ok(None) 空文件
    /// Err(()) 缓存未找到
    pub fn get_last_block(&self) -> Result<Option<(usize, CID)>, ()> {
        if self.cid_start.is_free() {
            return Ok(None);
        }
        debug_assert!(self.cid_start.is_next());
        if let Some(x) = self.len {
            if self.almost_last.0 == x - 1 {
                return Ok(Some(self.almost_last));
            }
        }
        Err(())
    }
    pub fn append_first(&mut self, cid: CID) {
        debug_assert!(self.cid_list.is_empty());
        debug_assert!(self.cid_start.is_free());
        debug_assert!(self.len.unwrap() == 0);
        debug_assert!(self.almost_last == (0, CID::FREE));
        self.cid_start = cid;
        self.short.set_cluster(cid);
        self.cid_list.push(cid);
        self.almost_last = (0, cid);
        self.len = Some(1);
    }
    /// 簇偏移 簇ID
    pub fn append_last(&mut self, n: usize, cid: CID) {
        debug_assert!(self.cid_start.is_next());
        if self.cid_list.len() == n {
            self.cid_list.push(cid);
        }
        self.almost_last = (n, cid);
        *self.len.as_mut().unwrap() += 1;
    }
}
