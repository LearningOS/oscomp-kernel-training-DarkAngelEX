use core::task::Waker;

use alloc::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    vec::Vec,
};
use ftl_util::{
    device::BlockDevice,
    error::{SysError, SysR},
};

use crate::{
    block::buffer::Buffer,
    block_dev::PanicBlockDevice,
    layout::bpb::RawBPB,
    mutex::{MultiplySemaphore, SemaphoreGuard, SpinMutex},
    tools::{AIDAllocator, AID, CID, SID},
    PRINT_BLOCK_OP,
};

use super::{bcache::Cache, buffer::SharedBuffer};

/// 此管理器仅用于获取块 不会进行任何读写操作 因此也不需要异步操作函数
pub(crate) struct CacheManagerInner {
    // 不可变数据
    max_cid: CID,                     // 簇数 list中超过size的将被忽略
    sector_bytes: usize,              // 扇区字节数
    cluster_bytes: usize,             // 簇字节数
    pub data_sector_start: SID,       // 数据区开始扇区
    pub sector_per_cluster_log2: u32, // 每个簇多少扇区
    max_cache_num: usize,             // 最大缓存块数

    aid_alloc: Arc<AIDAllocator>,
    search: BTreeMap<CID, (Arc<Cache>, AID)>, // 簇号 -> 块号 使用Arc来减少原子操作
    clean: BTreeMap<AID, (CID, Arc<Cache>)>,  // LRU 搜索替换搜索 与dirty保证无交集
    dirty: BTreeMap<CID, (Arc<Cache>, SemaphoreGuard)>, // 等待同步或运行在驱动的块
    pub sync_pending: Arc<SpinMutex<Option<BTreeSet<CID>>>>, // 同步系统优先获取的集合

    pub sync_waker: Option<Waker>,
    pub device: Arc<dyn BlockDevice>,
}

impl Drop for CacheManagerInner {
    fn drop(&mut self) {
        self.close();
    }
}

impl CacheManagerInner {
    pub fn new(max_cache_num: usize) -> Self {
        Self {
            max_cid: CID(0),
            sector_bytes: 0,
            cluster_bytes: 0,
            data_sector_start: SID(0),
            sector_per_cluster_log2: 0,
            max_cache_num,

            aid_alloc: Arc::new(AIDAllocator::new()),
            search: BTreeMap::new(),
            clean: BTreeMap::new(),
            dirty: BTreeMap::new(),
            sync_pending: Arc::new(SpinMutex::new(None)),

            sync_waker: None,
            device: Arc::new(PanicBlockDevice),
        }
    }
    pub async fn init(&mut self, bpb: &RawBPB, device: Arc<dyn BlockDevice>) {
        self.max_cid = CID(bpb.data_cluster_num as u32);
        self.sector_bytes = bpb.sector_bytes as usize;
        self.cluster_bytes = bpb.cluster_bytes;
        self.data_sector_start = bpb.data_sector_start;
        self.sector_per_cluster_log2 = bpb.sector_per_cluster.log2();

        *self.sync_pending.lock() = Some(BTreeSet::new());
        self.device = device;
    }
    pub fn close(&mut self) {
        self.sync_pending.lock().take();
        if self.sync_waker.is_some() {
            self.sync_waker().wake();
        }
    }
    pub fn set_waker(&mut self, waker: Waker) {
        self.sync_waker = Some(waker);
    }
    pub fn sync_waker(&self) -> Waker {
        self.sync_waker.as_ref().unwrap().clone()
    }
    pub fn wake_sync(&self) {
        self.sync_waker.as_ref().unwrap().wake_by_ref()
    }
    pub fn raw_get_sid_of_cid(start: SID, s_p_c_log2: u32, cid: CID) -> SID {
        SID(start.0 + ((cid.0 - 2) << s_p_c_log2))
    }
    pub fn get_sid_of_cid(&self, cid: CID) -> SID {
        Self::raw_get_sid_of_cid(self.data_sector_start, self.sector_per_cluster_log2, cid)
    }
    /// 如果缓存块不存在将从磁盘加载数据
    ///
    /// 如果替换了缓存块则返回它的CID
    pub async fn get_block(&mut self, cid: CID) -> SysR<(Arc<Cache>, Option<CID>)> {
        stack_trace!();
        debug_assert!(cid.0 >= 2 && cid < self.max_cid);
        if let Some((c, _aid)) = self.search.get(&cid) {
            return Ok((c.clone(), None));
        }
        let (mut cache, replace_cid) = self.get_new_uninit_block()?;
        stack_trace!();
        self.device
            .read_block(self.get_sid_of_cid(cid).0 as usize, cache.init_buffer()?)
            .await?;
        Ok((self.force_insert_block(cache, cid), replace_cid))
    }
    pub fn have_block_of(&self, cid: CID) -> bool {
        self.search.contains_key(&cid)
    }
    pub async fn get_dirty_shared_buffer(&mut self, cid: CID) -> SharedBuffer {
        self.dirty.get(&cid).unwrap().0.shared().await
    }
    /// 分配一个已经分配了内存但没有加载数据的cache
    ///
    /// 如果替换了一个块将返回它的CID
    pub fn get_new_uninit_block(&mut self) -> SysR<(Cache, Option<CID>)> {
        stack_trace!();
        if self.search.len() < self.max_cache_num {
            return Ok((Cache::new(Buffer::new(self.cluster_bytes)?), None));
        }
        assert!(!self.clean.is_empty());
        // 扫描结束判断
        let search_max = self.aid_alloc.alloc();
        loop {
            let (xaid, (cid, cache)) = self.clean.pop_first().unwrap();
            if xaid > search_max {
                // 全部FAT索引都被占用了! 320MB的缓存啊 8万个缓存块
                return Err(SysError::ENOBUFS);
            }
            if cache.aid() != xaid {
                let aid = cache.aid();
                self.search.get_mut(&cid).unwrap().1 = aid;
                self.clean.try_insert(aid, (cid, cache)).ok().unwrap();
                continue;
            }
            // 以下continue概率极低 原子操作都在这里
            // 两个强引用只会出现在 search 或 clean 极小概率所有权被某进程从Weak索引获取
            let (ps, xxaid) = self.search.remove(&cid).unwrap(); // 减少引用计数
            debug_assert_eq!(xaid, xxaid);
            debug_assert!(Arc::strong_count(&cache) >= 2);
            if Arc::strong_count(&cache) != 2 {
                let aid = self.aid_alloc.alloc();
                cache.update_aid(aid);
                self.search.try_insert(cid, (ps, aid)).ok().unwrap();
                self.clean.try_insert(aid, (cid, cache)).ok().unwrap();
                continue;
            }
            drop(ps);
            match Arc::try_unwrap(cache) {
                Err(cache) => {
                    let aid = self.aid_alloc.alloc();
                    cache.update_aid(aid);
                    self.search
                        .try_insert(cid, (cache.clone(), aid))
                        .ok()
                        .unwrap();
                    self.clean.try_insert(aid, (cid, cache)).ok().unwrap();
                    continue;
                }
                Ok(cache) => return Ok((cache, Some(cid))),
            }
        }
    }
    /// 从缓存块中释放块并取消同步任务
    pub fn release_block(&mut self, cid: CID) {
        let (_, a) = self.search.remove(&cid).unwrap();
        let _ = self.sync_pending.lock().as_mut().unwrap().remove(&cid);
        let _ = self.clean.remove(&a);
        let _ = self.dirty.remove(&cid);
    }
    /// 此函数会分配一个aid
    pub fn force_insert_block(&mut self, cache: Cache, cid: CID) -> Arc<Cache> {
        stack_trace!();
        if PRINT_BLOCK_OP {
            println!("force_insert_block: {:?}", cid);
        }
        let aid = self.aid_alloc.alloc();
        cache.update_aid(aid);
        let cache = Arc::new(cache);
        self.search
            .try_insert(cid, (cache.clone(), aid))
            .ok()
            .unwrap();
        self.clean
            .try_insert(aid, (cid, cache.clone()))
            .ok()
            .unwrap();
        cache
    }
    pub fn become_dirty(&mut self, cid: CID, sems: &mut MultiplySemaphore) {
        stack_trace!();
        debug_assert!(sems.val() >= 1);
        if PRINT_BLOCK_OP {
            println!("become_dirty: {:?}", cid);
        }
        let aid = self.search.get(&cid).unwrap().1;
        if let Some((xcid, c)) = self.clean.remove(&aid) {
            debug_assert!(cid == xcid);
            self.dirty
                .try_insert(cid, (c, sems.try_take().unwrap()))
                .ok()
                .unwrap();
            if !self.sync_pending.lock().as_mut().unwrap().insert(cid) {
                panic!();
            }
            self.wake_sync();
        } else {
            debug_assert!(self.dirty.contains_key(&cid));
            let ok = self.sync_pending.lock().as_mut().unwrap().insert(cid);
            if ok {
                self.wake_sync();
            }
        }
    }
    /// 由同步系统进行回调
    pub fn dirty_suspend_iter(&mut self, cid_iter: impl Iterator<Item = CID>) {
        let sync_pending = self.sync_pending.lock();
        let sync_pending = sync_pending.as_ref().unwrap();
        let mut set = Vec::new();
        for cid in cid_iter {
            if PRINT_BLOCK_OP {
                set.push(cid);
            }
            debug_assert!(self.dirty.contains_key(&cid));
            if sync_pending.contains(&cid) {
                continue;
            }
            let unit = self.dirty.remove(&cid).unwrap().0;
            let aid = self.aid_alloc.alloc();
            self.clean.try_insert(aid, (cid, unit)).ok().unwrap();
            self.search.get_mut(&cid).unwrap().1 = aid;
        }
        if PRINT_BLOCK_OP {
            println!("dirty_suspend: {:?}", set.as_slice());
        }
    }
    /// 尝试释放最久未访问的n个缓存块 返回实际释放的数量
    ///
    /// 释放的都为空闲缓存块, 即clean集合
    pub fn try_release_free(&mut self, n: usize) -> usize {
        stack_trace!();
        let mut cnt = 0;
        let search_max = self.aid_alloc.alloc();
        while cnt < n {
            let (xaid, (cid, cache)) = self.clean.pop_first().unwrap();
            if xaid > search_max {
                // 整个clean表扫了一遍
                return cnt;
            }
            if cache.aid() != xaid {
                self.clean
                    .try_insert(cache.aid(), (cid, cache))
                    .ok()
                    .unwrap();
                continue;
            }
            // 以下continue概率极低 原子操作都在这里
            // 两个强引用只会出现在 search 或 clean 极小概率所有权被某进程从Weak索引获取
            let (ps, xxaid) = self.search.remove(&cid).unwrap(); // 减少引用计数
            debug_assert_eq!(xaid, xxaid);
            debug_assert!(Arc::strong_count(&cache) >= 2);
            if Arc::strong_count(&cache) != 2 {
                let aid = self.aid_alloc.alloc();
                cache.update_aid(aid);
                self.search.try_insert(cid, (ps, aid)).ok().unwrap();
                self.clean.try_insert(aid, (cid, cache)).ok().unwrap();
                continue;
            }
            drop(ps);
            match Arc::try_unwrap(cache) {
                Err(cache) => {
                    let aid = self.aid_alloc.alloc();
                    cache.update_aid(aid);
                    self.search
                        .try_insert(cid, (cache.clone(), aid))
                        .ok()
                        .unwrap();
                    self.clean
                        .try_insert(cache.aid(), (cid, cache))
                        .ok()
                        .unwrap();
                    continue;
                }
                Ok(cache) => drop(cache),
            }
            cnt += 1;
        }
        cnt
    }
}
