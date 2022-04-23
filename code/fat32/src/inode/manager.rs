use alloc::{collections::BTreeMap, sync::Arc};

use crate::{
    mutex::rw_spin_mutex::RwSpinMutex,
    tools::{AIDAllocator, AID},
};

use super::{inode_cache::InodeCache, InodeMark, IID};

pub struct InodeManager {
    aid_alloc: Arc<AIDAllocator>,
    inner: RwSpinMutex<InodeManagerInner>,
}

impl InodeManager {
    pub fn new(target_free: usize) -> Self {
        let aid_alloc = Arc::new(AIDAllocator::new());
        let x = aid_alloc.clone();
        Self {
            aid_alloc,
            inner: RwSpinMutex::new(InodeManagerInner::new(x, target_free)),
        }
    }
    pub fn init(&mut self) {}
}

/// 每个打开的文件都会在这里缓存
///
/// 打开的文件将在此获取Inode, 如果Inode不存在则自动创建一个
///
/// Inode析构将抹去这里的记录 缓存将被动释放
pub struct InodeManagerInner {
    aid_alloc: Arc<AIDAllocator>,
    target_free: usize,    // 目标空闲缓存数量 空闲数超过这个值的两倍将释放一部分
    alive: Arc<InodeMark>, // 强引用计数-1即为打开的文件数量
    search: BTreeMap<IID, (AID, Arc<InodeCache>)>,
    access: BTreeMap<AID, (IID, Arc<InodeCache>)>,
}

impl InodeManagerInner {
    pub fn new(aid_alloc: Arc<AIDAllocator>, target_free: usize) -> Self {
        Self {
            aid_alloc,
            target_free,
            alive: Arc::new(InodeMark),
            search: BTreeMap::new(),
            access: BTreeMap::new(),
        }
    }
    pub fn init(&mut self) {}
    pub fn try_get_cache(&self, iid: IID) -> Option<Arc<InodeCache>> {
        self.search.get(&iid).map(|(_, v)| v.clone())
    }
    /// 此函数需要先在同一个锁下用try_get_cache检测失败后进行
    pub fn force_insert_cache(&mut self, iid: IID, ic: Arc<InodeCache>) {
        self.recycle();
        let aid = self.aid_alloc.alloc();
        ic.update_aid();
        self.search.try_insert(iid, (aid, ic.clone())).ok().unwrap();
        self.access.try_insert(aid, (iid, ic)).ok().unwrap();
    }
    /// 打开的文件数量
    pub fn file_opened_num(&self) -> usize {
        Arc::strong_count(&self.alive) - 1
    }
    /// 当空闲缓存数量达到目标缓存数开始释放LRU缓存
    pub fn recycle(&mut self) {
        let cur_free = self.search.len() - self.file_opened_num();
        if cur_free <= self.target_free {
            return;
        }
        let mut cnt = if cur_free <= self.target_free * 3 {
            (cur_free - self.target_free) / 2
        } else {
            cur_free - self.target_free * 2
        };
        let search_max = self.aid_alloc.alloc();
        while cnt > 0 {
            match self.recycle_one(search_max) {
                Ok(true) => cnt -= 1,
                Ok(false) => (),
                Err(()) => return,
            }
        }
    }
    /// 回收一个值 Ok(true) 成功释放 Ok(false) retry Err(()) 空
    pub fn recycle_one(&mut self, max_aid: AID) -> Result<bool, ()> {
        if self.access.is_empty() {
            return Err(());
        }
        let (xaid, (iid, ic)) = self.access.pop_first().unwrap();
        if xaid > max_aid {
            return Err(());
        }
        if xaid != ic.aid() {
            self.search.get_mut(&iid).unwrap().0 = ic.aid();
            self.access.try_insert(ic.aid(), (iid, ic)).ok().unwrap();
            return Ok(false);
        }
        let (xxaid, ps) = self.search.remove(&iid).unwrap(); // 减少引用计数
        debug_assert_eq!(xaid, xxaid);
        debug_assert!(Arc::strong_count(&ic) >= 2);
        if Arc::strong_count(&ic) != 2 {
            let aid = ic.update_aid();
            self.search.try_insert(iid, (aid, ps)).ok().unwrap();
            self.access.try_insert(aid, (iid, ic)).ok().unwrap();
            return Ok(false);
        }
        drop(ps);
        match Arc::try_unwrap(ic) {
            Err(ic) => {
                let aid = ic.update_aid();
                self.search.try_insert(iid, (aid, ic.clone())).ok().unwrap();
                self.access.try_insert(aid, (iid, ic)).ok().unwrap();
                return Ok(false);
            }
            Ok(ic) => drop(ic),
        }
        return Ok(true);
    }
}
