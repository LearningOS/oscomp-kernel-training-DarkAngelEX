use core::{
    future::Future,
    ops::{ControlFlow, Try},
    pin::Pin,
    sync::atomic::{AtomicUsize, Ordering},
    task::{Context, Poll},
};

use alloc::{boxed::Box, collections::BTreeSet, sync::Arc, vec::Vec};
use ftl_util::{device::BlockDevice, error::SysError};

use crate::{
    layout::bpb::RawBPB,
    mutex::{Semaphore, SleepMutex, SpinMutex},
    tools::{
        self,
        xasync::{GetWakerFuture, WaitSemFuture, WaitingEventFuture},
        AIDAllocator, CID,
    },
};

use self::{
    index::ListIndex,
    manager::ListManager,
    unit::{ListUnit, UnitID},
};

mod index;
mod manager;
mod unit;

/// FAT链表
///
/// 如果缓存存在只需要非常短暂地持有Weak指针锁upgrade为Arc
pub(crate) struct FatList {
    aid_alloc: Arc<AIDAllocator>,          // 分配访问号
    list_index: ListIndex,                 // 链表索引
    max_cid: CID,                          // 簇数 list中超过size的将被忽略
    max_unit_num: usize,                   // 最大索引块数量
    sector_bytes: usize,                   // 扇区大小
    u32_per_sector_log2: u32,              // 一个扇区可以放多少个u32
    dirty_semaphore: Semaphore,            // 脏块信号量 必须小于最大缓存数
    manager: Arc<SleepMutex<ListManager>>, // 全局管理系统 互斥操作
}

impl FatList {
    pub fn empty(max_dirty: usize, max_cache_num: usize) -> Self {
        let aid_alloc = Arc::new(AIDAllocator::new());
        Self {
            aid_alloc: aid_alloc.clone(),
            list_index: ListIndex::new(),
            max_cid: CID(0),
            sector_bytes: 0,
            u32_per_sector_log2: 0,
            max_unit_num: max_cache_num,
            dirty_semaphore: Semaphore::new(max_dirty),
            manager: Arc::new(SleepMutex::new(ListManager::new(aid_alloc, max_cache_num))),
        }
    }
    /// 加载第 n 个副本
    pub async fn init(&mut self, bpb: &RawBPB, n: usize, device: Arc<dyn BlockDevice>) {
        self.max_cid = CID(bpb.data_cluster_num as u32);
        self.sector_bytes = bpb.sector_bytes as usize;
        self.u32_per_sector_log2 = bpb.sector_bytes_log2 - core::mem::size_of::<u32>().log2();
        self.max_unit_num = (bpb.data_cluster_num + (1 << self.u32_per_sector_log2) - 1)
            >> self.u32_per_sector_log2;
        self.list_index.init(self.max_unit_num).unwrap();
        let manager = Arc::get_mut(&mut self.manager).unwrap().get_mut();
        manager.init(bpb, n, device).await;
    }
    /// 按扇区大小切分索引 (单元索引号, 单元偏移)
    fn sector_split(&self, sid: usize) -> (usize, usize) {
        let bit = self.u32_per_sector_log2;
        (sid >> bit, sid % (1 << bit))
    }
    fn unit_id_split(&self, uid: UnitID) -> (usize, usize) {
        self.sector_split(uid.0 as usize)
    }
    /// unit偏移 unit内偏移
    fn get_unit_of_cid(&self, cid: CID) -> (UnitID, usize) {
        let (a, b) = self.sector_split(cid.0 as usize);
        (UnitID(a as u32), b)
    }
    /// LRU替换一个旧的块
    async fn get_unit(&self, id: UnitID) -> Result<Arc<ListUnit>, SysError> {
        self.manager.lock().await.get_unit(id).await
    }
    pub async fn get_next(&self, cid: CID) -> Result<CID, SysError> {
        // debug_assert!(cid.is_next() && cid < self.max_cid);
        debug_assert!(cid < self.max_cid);
        let (off, i2) = self.get_unit_of_cid(cid);
        let (i0, _i1) = self.unit_id_split(off);
        let unit = match self.list_index.get(i0) {
            Some(unit) => unit,
            None => {
                let unit = self.get_unit(off).await?;
                self.list_index.set(i0, &unit);
                unit
            }
        };
        Ok(unit.get(i2, self.aid_alloc.alloc()))
    }
    /// 从起始块开始扫描FAT链表 如果存在缓存将伪无锁进行
    ///
    /// start_off是输入cid所在块的偏移量, op的第一次调用off将为start_off+1
    ///
    /// 假设输入cid=8 off=0 链表序列为 8->9->10->LAST 将调用: (B,9,1) (B,10,2) (B,LAST,3) break
    pub async fn travel<B>(
        &self,
        cid: CID, // start CID
        start_off: usize,
        init: B,
        mut op: impl FnMut(B, CID, usize) -> ControlFlow<B, B>,
    ) -> ControlFlow<Result<B, SysError>, B> {
        let mut cur = cid;
        let mut accum = init;
        let mut i = start_off + 1;
        let mut unit: Option<(UnitID, Arc<ListUnit>)> = None; // 缓存一个缓存块加速缓存内查找
        while cur.is_next() {
            let (uid, uoff) = self.get_unit_of_cid(cur);
            let unit_cur = match unit.take() {
                Some(unit) if unit.0 == uid => unit.1,
                _ => self
                    .get_unit(uid)
                    .await
                    .branch()
                    .map_break(tools::err_break)?,
            };
            let nxt = unit_cur.get(uoff, self.aid_alloc.alloc());
            accum = op(accum, nxt, i).map_break(Ok)?;
            unit = Some((uid, unit_cur));
            cur = nxt;
            i += 1;
        }
        try { accum }
    }
    pub async fn alloc_block(&self) -> Result<CID, SysError> {
        let sem = self.dirty_semaphore.take().await;
        self.manager.lock().await.alloc_cluster(sem).await
    }
    /// cid 必须是链表的最后一项, 即FAT链表NEXT为LAST
    pub async fn alloc_block_after(&self, cid: CID) -> Result<CID, SysError> {
        let mut sems = self.dirty_semaphore.take_n(2).await;
        self.manager
            .lock()
            .await
            .alloc_cluster_after(cid, &mut sems)
            .await
    }
    /// 释放CID对应的簇
    pub async fn free_cluster(&self, cid: CID) -> Result<(), SysError> {
        stack_trace!();
        debug_assert!(cid.is_next());
        let sem = self.dirty_semaphore.take().await;
        self.manager.lock().await.free_cluster(cid, sem).await
    }
    /// 释放从cid开始的整个链表 如果中途出错依然会保存链表的合法 完全释放返回Ok(())
    ///
    /// 不会释放cid本身, fat链表中cid将置为LAST
    ///
    /// 返回释放了多少个簇
    ///
    /// A -> B -> C -> D -> E 如果释放D时出错将变为 A -> D -> E
    pub async fn free_cluster_at(&self, cid: CID) -> (usize, Result<(), SysError>) {
        stack_trace!();
        debug_assert!(cid.is_next());
        let n = (self.dirty_semaphore.max() / 4).max(2).min(10);
        let mut free_n = 0;
        loop {
            let mut sems = self.dirty_semaphore.take_n(n).await;
            let manager = &mut *self.manager.lock().await;
            let (this_n, ret) = manager.free_cluster_at(cid, &mut sems).await;
            free_n += this_n;
            return match ret {
                Ok(Err(())) => continue,
                Ok(Ok(())) => (free_n, Ok(())),
                Err(e) => (free_n, Err(e)),
            };
        }
    }
    /// 并发同步系统 参数为最大并发任务数
    ///
    /// 必须将此函数spawn后将waker更新进manager.sync_waker
    pub async fn sync_task(
        &mut self,
        concurrent: usize,
        mut spawn_fn: impl FnMut(Pin<Box<dyn Future<Output = ()> + Send + 'static>>)
            + Clone
            + Send
            + 'static,
    ) {
        let init_manager = Arc::get_mut(&mut self.manager).unwrap().get_mut();
        let device = init_manager.device.clone();
        let sync_start = init_manager.store_start.clone();
        let sync = init_manager.sync_pending.clone();
        let info_cluster_id = init_manager.info_cluster_id;
        let manager = self.manager.clone();
        let mut spawn_fn_x = spawn_fn.clone();
        let this_waker = GetWakerFuture.await;
        let future = async move {
            let sem = Arc::new(AtomicUsize::new(concurrent));
            let waker = GetWakerFuture.await;
            manager.lock().await.set_waker(waker.clone());
            this_waker.wake();
            // fsinfo改变一定伴随着Dirty
            while let Ok(s) = WaitDirtyFuture(sync.clone()).await {
                let set: Vec<_> = {
                    let lock = &mut *manager.lock().await;
                    s.into_iter()
                        .map(|uid| (uid, lock.get_dirty_shared_buffer(uid)))
                        .collect()
                };
                for &start in &sync_start {
                    for (uid, buffer) in set.iter() {
                        WaitSemFuture(sem.as_ref()).await;
                        let uid = *uid;
                        let device = device.clone();
                        let sem = sem.clone();
                        let waker = waker.clone();
                        let buffer = buffer.clone();
                        let sid = ListManager::get_sid_of_unit_id(start, uid);
                        spawn_fn_x(Box::pin(async move {
                            device.write_block(sid.0 as usize, &*buffer).await.unwrap();
                            sem.fetch_add(1, Ordering::Relaxed);
                            waker.wake();
                        }));
                    }
                }
                manager
                    .lock()
                    .await
                    .dirty_suspend_iter(set.into_iter().map(|(a, _b)| a));
                let buffer = {
                    let manager = &mut *manager.lock().await;
                    if !manager.fsinfo_need_sync() {
                        continue;
                    }
                    manager.fsifo_store_buffer_device().unwrap()
                };
                WaitSemFuture(sem.as_ref()).await;
                let device = device.clone();
                let manager = manager.clone();
                let sem = sem.clone();
                let waker = waker.clone();
                spawn_fn_x(Box::pin(async move {
                    device.write_block(info_cluster_id, &*buffer).await.unwrap();
                    manager.lock().await.fsinfo_leave_device();
                    sem.fetch_add(1, Ordering::Relaxed);
                    waker.wake();
                }));
            }
        };
        spawn_fn(Box::pin(future));
        WaitingEventFuture(|| unsafe { self.manager.unsafe_get().sync_waker.as_ref().is_some() })
            .await;

        struct WaitDirtyFuture(Arc<SpinMutex<Option<BTreeSet<UnitID>>>>);
        impl Future for WaitDirtyFuture {
            type Output = Result<BTreeSet<UnitID>, ()>;
            fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
                match &mut *self.0.lock() {
                    Some(set) if set.is_empty() => Poll::Pending,
                    Some(set) => Poll::Ready(Ok(core::mem::take(set))),
                    None => Poll::Ready(Err(())), // Exit
                }
            }
        }
    }
    pub async fn show(&self, mut n: usize) {
        if n == 0 {
            n = usize::MAX;
        }
        for cid in (0..self.max_cid.0.min(n as u32)).map(CID) {
            let next = self.get_next(cid).await.unwrap();
            println!("{:>8X} -> {:>8X}", cid.0, next.0);
        }
    }
}
