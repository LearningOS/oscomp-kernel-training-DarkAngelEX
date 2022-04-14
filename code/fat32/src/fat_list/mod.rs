mod index;
mod inner;
mod unit;

use core::{
    future::Future,
    ops::{ControlFlow, Try},
    pin::Pin,
    sync::atomic::{AtomicUsize, Ordering},
    task::{Context, Poll},
};

use alloc::{boxed::Box, collections::BTreeSet, sync::Arc, vec::Vec};

use crate::{
    layout::bpb::RawBPB,
    mutex::{semaphore::Semaphore, sleep_mutex::SleepMutex, spin_mutex::SpinMutex},
    tools::{self, AIDAllocator, CID},
    xerror::SysError,
    BlockDevice,
};

use self::{
    index::ListIndex,
    inner::ListInner,
    unit::{ListUnit, UnitID},
};

/// FAT链表
///
/// 如果缓存存在只需要非常短暂地持有Weak指针锁upgrade为Arc
pub struct FatList {
    aid_alloc: Arc<AIDAllocator<ListUnit>>, // 分配访问号
    list_index: ListIndex,                  // 链表索引
    max_cid: CID,                           // 簇数 list中超过size的将被忽略
    max_unit_num: usize,                    // 最大索引块数量
    sector_bytes: usize,                    // 扇区大小
    u32_per_sector_log2: u32,               // 一个扇区可以放多少个u32
    dirty_semaphore: Semaphore,             // 脏块信号量 必须小于最大缓存数
    inner: Arc<SleepMutex<ListInner>>,      // 全局管理系统 互斥操作
}

impl FatList {
    pub fn empty(max_dirty: usize, max_unit_num: usize) -> Self {
        let aid_alloc = Arc::new(AIDAllocator::new());
        Self {
            aid_alloc: aid_alloc.clone(),
            list_index: ListIndex::new(),
            max_cid: CID(0),
            sector_bytes: 0,
            u32_per_sector_log2: 0,
            max_unit_num,
            dirty_semaphore: Semaphore::new(max_dirty),
            inner: Arc::new(SleepMutex::new(ListInner::new(aid_alloc, max_unit_num))),
        }
    }
    /// 加载第 n 个副本
    pub async fn init(&mut self, bpb: &RawBPB, n: usize, device: Arc<dyn BlockDevice>) {
        self.max_cid = CID(bpb.data_cluster_num as u32);
        self.sector_bytes = bpb.sector_bytes as usize;
        self.u32_per_sector_log2 = bpb.sector_bytes_log2 - core::mem::size_of::<u32>().log2();
        self.max_unit_num = bpb.data_cluster_num >> self.u32_per_sector_log2;
        self.list_index.init(self.max_unit_num).unwrap();
        let inner = Arc::get_mut(&mut self.inner).unwrap().get_mut();
        // let inner = self.inner.get_mut();
        inner.init(bpb, n, device).await;
    }
    /// 并发同步系统 参数为最大并发任务数
    ///
    /// 必须将此函数spawn后将waker更新进inner.sync_waker
    pub fn sync_task(
        &mut self,
        concurrent: usize,
        mut spawn_fn: impl FnMut(Box<dyn Future<Output = ()> + Send + 'static>) + Send + 'static,
    ) -> impl Future<Output = ()> + Send + 'static {
        let init_inner = Arc::get_mut(&mut self.inner).unwrap().get_mut();
        let device = init_inner.device.clone();
        let sync_start = init_inner.store_start.clone();
        let sync = init_inner.sync_pending.clone();
        let info_cluster_id = init_inner.info_cluster_id;
        let inner = self.inner.clone();
        return async move {
            let sem = Arc::new(AtomicUsize::new(0));
            while let Ok(s) = WaitDirtyFuture(sync.clone()).await {
                let mut lock = inner.lock().await;
                let waker = lock.sync_waker();
                let set: Vec<_> = s
                    .into_iter()
                    .map(|uid| (uid, lock.get_dirty_shared_buffer(uid)))
                    .collect();
                drop(lock);
                for &start in &sync_start {
                    for (uid, buffer) in set.iter() {
                        WaitLessFuture(sem.as_ref(), concurrent).await;
                        sem.fetch_add(1, Ordering::Relaxed);
                        let uid = *uid;
                        let device = device.clone();
                        let sem = sem.clone();
                        let waker = waker.clone();
                        let buffer = buffer.clone();
                        spawn_fn(Box::new(async move {
                            let sid = ListInner::get_sid_of_unit_id(start, uid);
                            device.write_block(sid.0 as usize, &*buffer).await.unwrap();
                            sem.fetch_sub(1, Ordering::Relaxed);
                            waker.wake();
                        }));
                    }
                }
                WaitLessFuture(sem.as_ref(), concurrent).await;
                let buffer = inner.lock().await.fsifo_store_buffer_device().unwrap();
                device.write_block(info_cluster_id, &*buffer).await.unwrap();
                inner.lock().await.fsinfo_leave_device();

            }
        };
        struct WaitDirtyFuture(Arc<SpinMutex<Option<BTreeSet<UnitID>>>>);
        impl Future for WaitDirtyFuture {
            type Output = Result<BTreeSet<UnitID>, ()>;
            fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
                match &mut *self.0.lock() {
                    Some(set) if set.is_empty() => Poll::Pending,
                    Some(set) => Poll::Ready(Ok(core::mem::take(set))),
                    None => Poll::Ready(Err(())),
                }
            }
        }

        struct WaitLessFuture<'a>(&'a AtomicUsize, usize);
        impl Future for WaitLessFuture<'_> {
            type Output = ();
            fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
                match self.0.load(Ordering::Relaxed) {
                    v if v <= self.1 => Poll::Ready(()),
                    _ => Poll::Pending,
                }
            }
        }
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
    /// LRU替换一个旧的块
    async fn get_unit(&self, id: UnitID) -> Result<Arc<ListUnit>, SysError> {
        self.inner.lock().await.get_unit(id).await
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
    /// 假设输入cid=8 链表序列为 8->9->10->LAST 将调用: (B,9,0) (B,10,1) (B,LAST,2) break
    pub async fn travel<B>(
        &self,
        cid: CID, // start CID
        init: B,
        mut op: impl FnMut(B, CID, usize) -> ControlFlow<Result<B, SysError>, B>,
    ) -> ControlFlow<Result<B, SysError>, B> {
        let mut cur = cid;
        let mut accum = init;
        let mut i = 0;
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
            accum = op(accum, nxt, i)?;
            unit = Some((uid, unit_cur));
            cur = nxt;
            i += 1;
        }
        try { accum }
    }
    /// 返回分配的簇号 这个簇将被写入FAT链表
    pub async fn alloc_cluster(&self) -> Result<CID, SysError> {
        let sem = self.dirty_semaphore.take().await;
        self.inner.lock().await.alloc_cluster(sem).await
    }
    /// 第offset个扇区对应的buffer
    fn get_buffer_of_sector<'a>(src: &'a [CID], bpb: &RawBPB, offset: usize) -> &'a [CID] {
        let per = bpb.sector_bytes as usize;
        &src[per * offset..per * (offset + 1)]
    }
    pub async fn alloc_block(&mut self) -> Result<CID, SysError> {
        let sem = self.dirty_semaphore.take().await;
        self.inner.lock().await.alloc_cluster(sem).await
    }
    pub async fn alloc_block_after(&mut self, cid: CID) -> Result<CID, SysError> {
        let mut sems = self.dirty_semaphore.take_n(2).await;
        self.inner.lock().await.alloc_cluster_after(cid, &mut sems).await
    }
    pub async fn show(&self, mut n: usize) {
        if n == 0 {
            n = usize::MAX;
        }
        for cid in (0..self.max_cid.0.min(n as u32)).map(CID) {
            let next = self.get_next(cid).await.unwrap();
            println!("{:>8X} -> {:>8X}", cid.0, next.0);
        }
        // for (i, &cid) in self.list.iter().take(self.size.min(n)).enumerate() {
        //     println!("{:>8X} -> {:>8X}", i, cid.0);
        // }
    }
}
