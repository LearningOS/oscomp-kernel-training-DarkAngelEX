use alloc::{
    string::String,
    sync::{Arc, Weak},
    vec::Vec,
};

use crate::{
    layout::name::{Attr, RawShortName},
    mutex::rw_spin_mutex::RwSpinMutex,
    tools::{AIDAllocator, Align8, SyncUnsafeCell, AID, CID},
};

use super::{raw_inode::RawInode, InodeMark, IID};

pub struct PlaceInfo {
    pub parent: Weak<RwSpinMutex<InodeCache>>, // 父目录缓存指针
    pub parent_iid: IID,                       // 父目录inode ID
    pub entry_cid: CID,                        // 此文件的短文件名所在的簇号
    pub entry_off: usize,                      // 簇内目录项偏移
}

/// 此缓存块的全部操作都是同步操作
pub struct InodeCache {
    pub inner: RwSpinMutex<InodeCacheInner>,
    aid_alloc: Arc<AIDAllocator>,
    pub aid: SyncUnsafeCell<AID>,
}

pub struct InodeCacheInner {
    pub place_info: PlaceInfo,     // 根目录是None
    pub inode: Weak<RawInode>,     // inode
    pub iid: IID,                  // 根目录为0
    pub cid_list: Vec<CID>,        // FAT链表缓存 所有CID都是有效的
    pub cid_start: CID,            // short中的首簇号 空文件是CID(0)
    pub almost_last: (usize, CID), // 缓存访问到的最后一个有效块 空文件为0 CID(0) 当.1为0时无效
    pub len: Option<usize>,        // 文件簇数
    pub name: String,
    pub short: Align8<RawShortName>,
}

impl InodeCache {
    pub fn new(
        iid: IID,
        name: String,
        short: Align8<RawShortName>,
        place: PlaceInfo,
        aid_alloc: Arc<AIDAllocator>,
    ) -> Self {
        Self {
            inner: RwSpinMutex::new(InodeCacheInner {
                place_info: place,
                inode: Weak::new(),
                iid,
                cid_list: Vec::new(),
                cid_start: short.cid(),
                almost_last: (0, short.cid()),
                len: None,
                name,
                short,
            }),
            aid_alloc,
            aid: SyncUnsafeCell::new(AID(0)),
        }
    }
    pub fn init(&mut self, iid: IID, name: String, short: &Align8<RawShortName>, place: PlaceInfo) {
        let inner = self.inner_mut();
        debug_assert!(inner.inode.strong_count() == 0);
        inner.iid = iid;
        inner.place_info = place;
        inner.cid_list = Vec::new();
        inner.almost_last = (0, short.cid());
        inner.cid_start = short.cid();
        if inner.cid_start.is_next() {
            inner.cid_list.push(inner.cid_start);
        } else {
            inner.len = Some(0);
        }
        inner.name = name;
        inner.short = *short;
    }
    pub fn inner_mut(&mut self) -> &mut InodeCacheInner {
        self.inner.get_mut()
    }
    /// 尝试快速获取inode
    pub fn try_get_inode(self: &Arc<Self>) -> Option<Arc<RawInode>> {
        self.inner.try_shared_lock()?.inode.upgrade()
    }
    pub fn get_inode(
        self: &Arc<Self>,
        parent: Arc<InodeCache>,
        mark: Arc<InodeMark>,
    ) -> Arc<RawInode> {
        let mut lock = self.inner.unique_lock();
        if let Some(i) = lock.inode.upgrade() {
            return i;
        }
        let inode = RawInode::new(self.clone(), parent, mark);
        let inode = Arc::new(inode);
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
}

impl InodeCacheInner {
    pub fn attr(&self) -> Attr {
        self.short.attributes
    }
    pub fn update_list(&mut self, cid: CID, n: usize) {
        if self.cid_list.len() > n {
            debug_assert!(self.cid_list[n] == cid);
            return;
        }
        debug_assert!(self.cid_list.len() == n);
        if self.almost_last.0 <= n {
            self.almost_last = (n, cid);
        }
        if !cid.is_next() {
            self.len = Some(n);
        } else {
            self.cid_list.push(cid);
        }
    }
    /// list的长度变为至多n
    pub fn list_truncate(&mut self, n: usize) {
        if n == 0 {
            self.cid_start = CID(0);
            self.cid_list.clear();
            self.almost_last = (0, CID::free());
            self.len = Some(0);
            return;
        }
        self.cid_list.truncate(n);
        self.almost_last = (self.cid_list.len() - 1, *self.cid_list.last().unwrap());
        self.len = Some(n);
    }
    /// 返回缓存的最后一个簇偏移与ID
    pub fn list_last_save(&self) -> Option<(usize, CID)> {
        if !self.cid_start.is_free() {
            debug_assert!(!self.cid_start.is_next());
            debug_assert!(self.almost_last == (0, CID::free()));
            return None;
        }
        debug_assert!(self.almost_last.0 > 0);
        debug_assert!(self.almost_last.1.is_next());
        Some(self.almost_last)
    }
    /// Some(Ok) 找到结果 保证是有效CID
    /// Some(Err) 链表长度不足 返回链表长度
    /// None 缓存不足
    pub fn try_get_nth_block_cid(&self, n: usize) -> Option<Result<CID, usize>> {
        if self.cid_start.is_free() {
            debug_assert!(!self.cid_start.is_next());
            debug_assert!(self.almost_last == (0, CID::free()));
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

        return Some(ret);
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
        return Err(());
    }
    pub fn append_first(&mut self, cid: CID) {
        debug_assert!(self.cid_list.is_empty());
        debug_assert!(self.cid_start.is_free());
        debug_assert!(self.len.unwrap() == 0);
        debug_assert!(self.almost_last == (0, CID::free()));
        self.cid_start = cid;
        self.cid_list.push(cid);
        self.almost_last = (0, cid);
        self.len = Some(1);
    }
    pub fn append_last(&mut self, n: usize, cid: CID) {
        debug_assert!(!self.cid_start.is_free());
        debug_assert!(self.len.unwrap() == n);
        debug_assert!(self.almost_last.0 == n - 1);
        if self.cid_list.len() == n {
            self.cid_list.push(cid);
        }
        self.almost_last = (n, cid);
        *self.len.as_mut().unwrap() += 1;
    }
}
