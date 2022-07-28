//! FAT链表全局管理系统 需要睡眠锁保护
use core::{ops::DerefMut, task::Waker};

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
    block::buffer::{Buffer, SharedBuffer},
    block_dev::PanicBlockDevice,
    layout::{bpb::RawBPB, fsinfo::RawFsInfo},
    mutex::{MultiplySemaphore, SemaphoreGuard, SpinMutex},
    tools::{AIDAllocator, AID, CID, SID},
};

use super::unit::{ListUnit, UnitID};

pub enum FsinfoStatus {
    Clean,     // 无需同步
    Dirty,     // 需要同步 任务未发送
    SyncClean, // 无需同步 正在同步
    SyncDirty, // 需要同步 正在同步
}

pub(crate) struct ListManager {
    // 不可变数据
    max_cid: CID,              // 簇数 list中超过size的将被忽略
    max_unit_num: usize,       // 最大缓存索引块数量
    sector_bytes: usize,       // 扇区大小
    u32_per_sector_log2: u32,  // 一个扇区可以放多少个u32
    load_start: SID,           // 加载数据使用的FAT表起始扇区号
    sector_per_fat: usize,     // 这个FAT表有多少个扇区
    pub store_start: Vec<SID>, // 同步数据的起始扇区号组

    // 可变数据
    aid_alloc: Arc<AIDAllocator>, // 分配访问号
    pub info_cluster_id: usize,   // fsinfo所在扇区
    fsinfo_cache: Option<Buffer>, // fsinfo缓存
    fsinfo_status: FsinfoStatus,  // fsinfo状态
    cluster_free: u32,            // 空簇数
    cluster_search: CID,          // 分配新的块开始搜索位置
    // 缓存块替换部分
    search: BTreeMap<UnitID, (AID, Arc<ListUnit>)>, // 扇区偏移量 -> 缓存块 使用Arc来减少原子操作
    clean: BTreeMap<AID, (UnitID, Arc<ListUnit>)>,  // LRU 搜索替换搜索 与dirty保证无交集
    dirty: BTreeMap<UnitID, (Arc<ListUnit>, SemaphoreGuard)>, // 等待同步或运行在驱动的块
    pub sync_pending: Arc<SpinMutex<Option<BTreeSet<UnitID>>>>, // 同步系统优先获取的集合

    pub sync_waker: Option<Waker>,
    pub device: Arc<dyn BlockDevice>,
}

impl Drop for ListManager {
    fn drop(&mut self) {
        self.close();
    }
}

impl ListManager {
    pub fn new(aid_alloc: Arc<AIDAllocator>, max_unit_num: usize) -> Self {
        Self {
            max_cid: CID(0),
            max_unit_num,
            sector_bytes: 0,
            u32_per_sector_log2: 0,
            load_start: SID(0),
            sector_per_fat: 0,
            store_start: Vec::new(),

            aid_alloc,
            info_cluster_id: 0,
            fsinfo_cache: None,
            fsinfo_status: FsinfoStatus::Clean,
            cluster_free: 0,
            cluster_search: CID(0),
            search: BTreeMap::new(),
            clean: BTreeMap::new(),
            dirty: BTreeMap::new(),
            sync_pending: Arc::new(SpinMutex::new(None)),
            sync_waker: None,
            device: Arc::new(PanicBlockDevice),
        }
    }
    pub async fn init(&mut self, bpb: &RawBPB, n: usize, device: Arc<dyn BlockDevice>) {
        stack_trace!();
        assert!(n < bpb.fat_num as usize);
        *self.sync_pending.lock() = Some(BTreeSet::new());
        self.sector_per_fat = bpb.sector_per_fat as usize;
        self.load_start = SID(bpb.fat_sector_start.0 as u32 + bpb.sector_per_fat * n as u32);
        for i in 0..bpb.fat_num {
            let v = SID(bpb.fat_sector_start.0 as u32 + bpb.sector_per_fat * i as u32);
            self.store_start.push(v);
        }
        self.max_cid = CID(bpb.data_cluster_num as u32);
        self.sector_bytes = bpb.sector_bytes as usize;
        self.u32_per_sector_log2 = bpb.sector_bytes_log2 - core::mem::size_of::<u32>().log2();
        self.info_cluster_id = bpb.info_cluster_id as usize;
        self.device = device;
        self.fsinfo_cache = Some(Buffer::new(bpb.sector_bytes as usize).unwrap());
        let fsinfo_cache = self.fsinfo_cache.as_mut().unwrap().access_rw_u8().unwrap();
        self.device
            .read_block(self.info_cluster_id, fsinfo_cache)
            .await
            .unwrap();
        let mut fsinfo = RawFsInfo::zeroed();
        fsinfo.raw_load(fsinfo_cache);
        self.fsinfo_status = FsinfoStatus::Clean;
        self.cluster_free = fsinfo.cluster_free;
        self.cluster_search = CID(fsinfo.cluster_next);
    }
    pub fn close(&mut self) {
        self.sync_pending.lock().take();
        if self.sync_waker.is_some() {
            self.sync_waker().wake();
        }
    }
    /// waker由FatList的sync_task生成
    pub fn set_waker(&mut self, waker: Waker) {
        self.sync_waker = Some(waker);
    }
    pub fn sync_waker(&self) -> Waker {
        self.sync_waker.as_ref().unwrap().clone()
    }
    /// 按扇区大小切分索引 (单元索引号, 单元偏移)
    fn sector_split(&self, sid: usize) -> (usize, usize) {
        let bit = self.u32_per_sector_log2;
        (sid >> bit, sid % (1 << bit))
    }
    fn unit_id_split(&self, uid: UnitID) -> (usize, usize) {
        self.sector_split(uid.0 as usize)
    }
    /// 访问某个cid的三级索引 (单元索引号, 单元偏移, 单元内偏移)
    fn get_index_of_cid(&self, cid: CID) -> (usize, usize, usize) {
        let bit = self.u32_per_sector_log2;
        let fat_sid = cid.0 as usize >> bit;
        let (i0, i1) = self.sector_split(fat_sid);
        (i0, i1, cid.0 as usize % (1 << bit))
    }
    /// unit偏移 unit内偏移
    fn get_unit_of_cid(&self, cid: CID) -> (UnitID, usize) {
        let (a, b) = self.sector_split(cid.0 as usize);
        (UnitID(a as u32), b)
    }
    pub fn get_sid_of_unit_id(start: SID, uid: UnitID) -> SID {
        SID(start.0 + uid.0)
    }
    pub fn fsinfo_need_sync(&self) -> bool {
        match self.fsinfo_status {
            FsinfoStatus::Dirty => true,
            FsinfoStatus::Clean | FsinfoStatus::SyncClean | FsinfoStatus::SyncDirty => false,
        }
    }
    fn fsinfo_into_dirty(&mut self) {
        self.fsinfo_status = match self.fsinfo_status {
            FsinfoStatus::Clean | FsinfoStatus::Dirty => FsinfoStatus::Dirty,
            FsinfoStatus::SyncClean | FsinfoStatus::SyncDirty => FsinfoStatus::SyncDirty,
        }
    }
    fn fsinfo_into_device(&mut self) {
        self.fsinfo_status = match self.fsinfo_status {
            FsinfoStatus::Dirty => FsinfoStatus::SyncClean,
            FsinfoStatus::Clean | FsinfoStatus::SyncClean | FsinfoStatus::SyncDirty => panic!(),
        }
    }
    pub fn fsinfo_leave_device(&mut self) {
        self.fsinfo_status = match self.fsinfo_status {
            FsinfoStatus::Clean | FsinfoStatus::Dirty => panic!(),
            FsinfoStatus::SyncClean => FsinfoStatus::Clean,
            FsinfoStatus::SyncDirty => FsinfoStatus::Dirty,
        }
    }
    pub fn fsifo_store_buffer_device(&mut self) -> SysR<SharedBuffer> {
        let buffer = self.fsinfo_cache.as_mut().unwrap().access_rw_u8()?;
        debug_assert!(buffer.len() >= 512);
        RawFsInfo::raw_store(self.cluster_free, self.cluster_search.0, buffer);
        self.fsinfo_into_device();
        Ok(self.fsinfo_cache.as_mut().unwrap().share())
    }
    fn unit_into_dirty(&mut self, uid: UnitID, sems: &mut MultiplySemaphore) {
        assert!(sems.val() >= 1);
        if self.dirty.contains_key(&uid) {
            self.sync_pending.lock().as_mut().unwrap().insert(uid);
        } else {
            let aid = self.search.get(&uid).unwrap().0;
            let (xuid, unit) = self.clean.remove(&aid).unwrap();
            debug_assert!(uid == xuid);
            self.dirty
                .try_insert(uid, (unit, sems.try_take().unwrap()))
                .ok()
                .unwrap();
            if !self.sync_pending.lock().as_mut().unwrap().insert(uid) {
                panic!();
            }
            if self.dirty.len() == 1 {
                self.sync_waker.as_ref().unwrap().wake_by_ref()
            }
        }
    }
    pub fn take_dirty_pending(&mut self) -> BTreeSet<UnitID> {
        core::mem::take(self.sync_pending.lock().as_mut().unwrap().deref_mut())
    }
    pub fn dirty_suspend(&mut self, uid: UnitID) {
        self.dirty_suspend_iter([uid].into_iter());
    }
    pub fn dirty_suspend_iter(&mut self, uid_iter: impl Iterator<Item = UnitID>) {
        let sync_pending = self.sync_pending.lock();
        let sync_pending = sync_pending.as_ref().unwrap();
        for uid in uid_iter {
            debug_assert!(self.dirty.contains_key(&uid));
            if !sync_pending.contains(&uid) {
                let unit = self.dirty.remove(&uid).unwrap().0;
                let aid = self.aid_alloc.alloc();
                self.clean.try_insert(aid, (uid, unit)).ok().unwrap();
                self.search.get_mut(&uid).unwrap().0 = aid;
            }
        }
    }
    pub fn get_dirty_shared_buffer(&mut self, uid: UnitID) -> SharedBuffer {
        self.dirty.get(&uid).unwrap().0.shared()
    }
    /// 此函数不会更新aid
    ///
    /// 如果找不到则LRU替换一个旧的块
    pub async fn get_unit(&mut self, uid: UnitID) -> SysR<Arc<ListUnit>> {
        stack_trace!();
        debug_assert!(
            uid.0 < self.sector_per_fat as u32,
            "{:?}",
            (uid, self.u32_per_sector_log2, self.sector_per_fat)
        );
        if let Some((_aid, unit)) = self.search.get(&uid) {
            return Ok(unit.clone());
        }
        let mut unit = self.get_new_uninit_unit()?;

        let block_id = self.load_start.0 + uid.0;
        self.device
            .read_block(block_id as usize, unit.init_load())
            .await?;
        let aid = self.aid_alloc.alloc();
        unit.update_aid(aid);
        let unit = Arc::new(unit);
        self.search
            .try_insert(uid, (aid, unit.clone()))
            .ok()
            .unwrap();
        self.clean
            .try_insert(aid, (uid, unit.clone()))
            .ok()
            .unwrap();
        Ok(unit)
    }
    /// 分配一个已经分配了内存但没有加载数据的unit
    ///
    /// 如果找不到则LRU替换一个旧的块
    fn get_new_uninit_unit(&mut self) -> SysR<ListUnit> {
        stack_trace!();
        if self.search.len() < self.max_unit_num {
            return ListUnit::new_uninit(self.sector_bytes);
        }
        assert!(!self.clean.is_empty());
        // 扫描结束判断
        let search_max = self.aid_alloc.alloc();
        loop {
            let (xaid, (uid, unit)) = self.clean.pop_first().unwrap();
            if xaid > search_max {
                // 全部FAT索引都被占用了! 320MB的缓存啊 8万个缓存块
                return Err(SysError::ENOBUFS);
            }
            if unit.aid() != xaid {
                let aid = unit.aid();
                self.search.get_mut(&uid).unwrap().0 = aid;
                self.clean.try_insert(aid, (uid, unit)).ok().unwrap();
                continue;
            }
            // 以下continue概率极低 原子操作都在这里
            // 两个强引用只会出现在 search 或 clean 极小概率所有权被某进程从Weak索引获取
            let (xxaid, ps) = self.search.remove(&uid).unwrap(); // 减少引用计数
            debug_assert_eq!(xaid, xxaid);
            debug_assert!(Arc::strong_count(&unit) >= 2);
            if Arc::strong_count(&unit) != 2 {
                let aid = self.aid_alloc.alloc();
                unit.update_aid(aid);
                self.search.try_insert(uid, (aid, ps)).ok().unwrap();
                self.clean.try_insert(aid, (uid, unit)).ok().unwrap();
                continue;
            }
            drop(ps);
            match Arc::try_unwrap(unit) {
                Err(unit) => {
                    let aid = self.aid_alloc.alloc();
                    unit.update_aid(aid);
                    self.search
                        .try_insert(uid, (aid, unit.clone()))
                        .ok()
                        .unwrap();
                    self.clean.try_insert(aid, (uid, unit)).ok().unwrap();
                    continue;
                }
                Ok(unit) => return Ok(unit),
            }
        }
    }
    pub async fn alloc_cluster(&mut self, sem: SemaphoreGuard) -> SysR<CID> {
        stack_trace!();
        if self.cluster_free == 0 {
            return Err(SysError::ENOSPC);
        }
        let mut uid = self.get_unit_of_cid(self.cluster_search).0;
        let mut unit = self.get_unit(uid).await?;
        let mut cnt = 0;
        let off = 'outer: loop {
            // 扫描整个盘最多2遍 (经过FAT首扇区两次)
            if uid.0 == 0 {
                if cnt == 2 {
                    panic!(); // fsinfo出错
                }
                cnt += 1;
            }
            for (off, &x) in unit.buffer_ro().iter().enumerate() {
                if (uid.0 << self.u32_per_sector_log2) + off as u32 >= self.max_cid.0 {
                    break;
                }
                if x.is_free() {
                    break 'outer off;
                }
            }
            uid = UnitID(uid.0 + 1);
            if (uid.0 << self.u32_per_sector_log2) >= self.max_cid.0 {
                uid = UnitID(0);
            }
            self.fsinfo_into_dirty();
            self.cluster_search = CID((uid.0 as u32) << self.u32_per_sector_log2);
            unit = self.get_unit(uid).await?;
        };
        unsafe { unit.set(off, CID::LAST)? };
        self.cluster_free -= 1;
        self.fsinfo_into_dirty();
        self.unit_into_dirty(uid, &mut sem.into_multiply());
        Ok(CID((uid.0 << self.u32_per_sector_log2) + off as u32))
    }
    /// 需要保证信号量容量不小于2
    ///
    /// debug将检测是否cid为链表的最后一项
    pub async fn alloc_cluster_after(
        &mut self,
        cid: CID,
        sems: &mut MultiplySemaphore,
    ) -> SysR<CID> {
        debug_assert!(sems.val() >= 2);
        debug_assert!(cid.is_next());
        let (uid, uoff) = self.get_unit_of_cid(cid);
        let blk = self.get_unit(uid).await?;
        blk.update_aid(self.aid_alloc.alloc());
        debug_assert!(blk.raw_get(uoff).is_last());
        unsafe { blk.to_unique() }?;
        let cid = self.alloc_cluster(sems.try_take().unwrap()).await?;
        unsafe { blk.set(uoff, cid).unwrap() }; // 由to_unique保证成功
        self.unit_into_dirty(uid, sems);
        Ok(cid)
    }
    /// 释放cid自身并置为 CID::free
    pub async fn free_cluster(&mut self, cid: CID, sem: SemaphoreGuard) -> SysR<()> {
        let (uid, uoff) = self.get_unit_of_cid(cid);
        let unit = self.get_unit(uid).await?;
        unit.update_aid(self.aid_alloc.alloc());
        unsafe { unit.set(uoff, CID::FREE)? };
        self.cluster_free += 1;
        self.fsinfo_into_dirty();
        self.unit_into_dirty(uid, &mut sem.into_multiply());
        Ok(())
    }
    /// 释放从CID开始的块 当信号量耗尽时返回Ok(Err) 需要重新获取信号量
    ///
    /// 不会释放cid自身, 成功时链表中cid对应位变为 CID::last
    ///
    /// 失败时将重置链表末尾
    pub async fn free_cluster_at(
        &mut self,
        cid: CID,
        sems: &mut MultiplySemaphore,
    ) -> (usize, SysR<Result<(), ()>>) {
        assert!(sems.val() >= 2);
        let sem = sems.try_take().unwrap();
        let (uid, uoff) = self.get_unit_of_cid(cid);
        let unit = match self.get_unit(uid).await {
            Err(e) => return (0, Err(e)),
            Ok(u) => u,
        };
        if let Err(e) = unsafe { unit.to_unique() } {
            return (0, Err(e));
        }
        let next_cid = unit.get(uoff, self.aid_alloc.alloc());
        match self.free_cluster_at_impl(next_cid, sems).await {
            Ok(free_n) => unsafe {
                unit.set(uoff, CID::LAST).unwrap();
                self.cluster_free += free_n as u32;
                self.fsinfo_into_dirty();
                self.unit_into_dirty(uid, &mut sem.into_multiply());
                (free_n, Ok(Ok(())))
            },
            Err((cid, free_n, e)) => unsafe {
                unit.set(uoff, cid).unwrap();
                self.cluster_free += free_n as u32;
                self.fsinfo_into_dirty();
                self.unit_into_dirty(uid, &mut sem.into_multiply());
                match e {
                    Ok(()) => (free_n, Ok(Err(()))),
                    Err(e) => (free_n, Err(e)),
                }
            },
        }
    }
    async fn free_cluster_at_impl(
        &mut self,
        mut cid: CID,
        sems: &mut MultiplySemaphore,
    ) -> Result<usize, (CID, usize, Result<(), SysError>)> {
        let mut cnt = 0;
        while cid.is_next() {
            if sems.val() == 0 {
                return Err((cid, cnt, Ok(())));
            }
            let (uid, uoff) = self.get_unit_of_cid(cid);
            let unit = self.get_unit(uid).await.map_err(|e| (cid, cnt, Err(e)))?;
            let next_cid = unit.raw_get(uoff);
            unsafe {
                unit.set(uoff, CID::FREE).map_err(|e| (cid, cnt, Err(e)))?;
                self.unit_into_dirty(uid, sems);
            }
            cid = next_cid;
            cnt += 1;
        }
        Ok(cnt)
    }
}
