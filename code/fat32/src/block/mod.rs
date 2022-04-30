use core::{
    future::Future,
    pin::Pin,
    sync::atomic::{AtomicUsize, Ordering},
    task::{Context, Poll, Waker},
};

use alloc::{boxed::Box, collections::BTreeSet, sync::Arc};

use crate::{
    layout::bpb::RawBPB,
    mutex::{semaphore::Semaphore, sleep_mutex::SleepMutex, spin_mutex::SpinMutex},
    tools::{
        xasync::{GetWakerFuture, WaitSemFuture, WaitingEventFuture},
        CID,
    },
    xerror::SysError,
    BlockDevice,
};

use self::{bcache::Cache, index::CacheIndex, inner::CacheManagerInner};

pub mod bcache;
pub mod buffer;
mod index;
mod inner;

pub(crate) struct CacheManager {
    index: CacheIndex,          // 无竞争索引
    dirty_semaphore: Semaphore, // 脏块信号量 必须小于最大缓存数
    inner: Arc<SleepMutex<CacheManagerInner>>,
}

impl CacheManager {
    pub fn new(max_dirty: usize, max_cache_num: usize) -> Self {
        Self {
            index: CacheIndex::new(),
            dirty_semaphore: Semaphore::new(max_dirty),
            inner: Arc::new(SleepMutex::new(CacheManagerInner::new(max_cache_num))),
        }
    }
    pub async fn init(&mut self, bpb: &RawBPB, device: Arc<dyn BlockDevice>) {
        Arc::get_mut(&mut self.inner)
            .unwrap()
            .get_mut()
            .init(bpb, device)
            .await;
    }
    pub async fn set_waker(&mut self, waker: Waker) {
        self.inner.lock().await.set_waker(waker)
    }
    /// 获取簇号对应的缓存块
    pub async fn get_block(&self, cid: CID) -> Result<Arc<Cache>, SysError> {
        debug_assert!(cid.is_next());
        if let Some(c) = self.index.get(cid) {
            return Ok(c);
        }
        let c = self.inner.lock().await.get_block(cid).await?;
        self.index.insert(cid, Arc::downgrade(&c));
        Ok(c)
    }
    /// 不从磁盘加载数据 而是使用init函数初始化
    pub async fn get_block_init<T: Copy>(
        &self,
        cid: CID,
        init: impl FnOnce(&mut [T]),
    ) -> Result<Arc<Cache>, SysError> {
        let mut blk = {
            let inner = &mut *self.inner.lock().await;
            // debug_assert!(!inner.have_block_of(cid));
            if inner.have_block_of(cid) {
                inner.release_block(cid);
            }
            inner.get_new_uninit_block()?
        };
        let sem = self.dirty_semaphore.take().await;
        init(blk.init_buffer()?);
        let inner = &mut *self.inner.lock().await;
        let c = inner.force_insert_block(blk, cid);
        inner.become_dirty(cid, &mut sem.into_multiply());
        Ok(c)
    }
    pub async fn write_block<T: Copy, V>(
        &self,
        cid: CID,
        cache: &Cache,
        op: impl FnOnce(&mut [T]) -> V,
    ) -> Result<V, SysError> {
        let sem = self.dirty_semaphore.take().await;
        let r = cache.access_rw(op).await?;
        self.inner
            .lock()
            .await
            .become_dirty(cid, &mut sem.into_multiply());
        Ok(r)
    }
    /// 从缓存块中释放块并取消同步任务
    pub async fn release_block(&self, cid: CID) {
        self.inner.lock().await.release_block(cid)
    }
    /// 生成一个同步任务
    pub async fn sync_task(
        &mut self,
        concurrent: usize,
        mut spawn_fn: impl FnMut(Pin<Box<dyn Future<Output = ()> + Send + 'static>>)
            + Clone
            + Send
            + 'static,
    ) {
        // 这一行保证了同步任务只会生成一次
        let init_inner = Arc::get_mut(&mut self.inner).unwrap().get_mut();
        let device = init_inner.device.clone();
        let data_sector_start = init_inner.data_sector_start;
        let spcl2 = init_inner.sector_per_cluster_log2;
        let sync = init_inner.sync_pending.clone();
        let manager = self.inner.clone();
        let mut spawn_fn_x = spawn_fn.clone();
        let this_waker = GetWakerFuture.await;
        let future = async move {
            let waker = GetWakerFuture.await;
            manager.lock().await.set_waker(waker.clone());
            this_waker.wake();
            let sem = Arc::new(AtomicUsize::new(concurrent));
            while let Ok(s) = WaitDirtyFuture(sync.clone()).await {
                for &cid in s.iter() {
                    let buffer = manager.lock().await.get_dirty_shared_buffer(cid).await;
                    WaitSemFuture(sem.as_ref()).await;
                    let device = device.clone();
                    let sem = sem.clone();
                    let waker = waker.clone();
                    let buffer = buffer.clone();
                    let sid = CacheManagerInner::raw_get_sid_of_cid(data_sector_start, spcl2, cid);
                    spawn_fn_x(Box::pin(async move {
                        device.write_block(sid.0 as usize, &*buffer).await.unwrap();
                        sem.fetch_add(1, Ordering::Relaxed);
                        waker.wake();
                    }));
                }
                manager.lock().await.dirty_suspend_iter(s.into_iter());
            }
        };
        spawn_fn(Box::pin(future));
        WaitingEventFuture(|| unsafe { self.inner.unsafe_get().sync_waker.as_ref().is_some() })
            .await;

        struct WaitDirtyFuture(Arc<SpinMutex<Option<BTreeSet<CID>>>>);
        impl Future for WaitDirtyFuture {
            type Output = Result<BTreeSet<CID>, ()>;
            fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
                match &mut *self.0.lock() {
                    Some(set) if set.is_empty() => Poll::Pending,
                    Some(set) => Poll::Ready(Ok(core::mem::take(set))),
                    None => Poll::Ready(Err(())), // Exit
                }
            }
        }
    }
}
