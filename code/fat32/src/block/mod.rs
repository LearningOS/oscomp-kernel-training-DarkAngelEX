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
    tools::CID,
    xerror::SysError,
    BlockDevice,
};

use self::{bcache::Cache, index::CacheIndex, inner::CacheManagerInner};

pub mod bcache;
pub mod buffer;
mod index;
mod inner;

pub struct CacheManager {
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
    /// 获取扇区对应的缓存块
    pub async fn get_block(&self, cid: CID) -> Result<Arc<Cache>, SysError> {
        if let Some(b) = self.index.get(cid) {
            return Ok(b);
        }
        let b = self.inner.lock().await.get_block(cid).await?;
        self.index.insert(cid, Arc::downgrade(&b));
        Ok(b)
    }
    /// 不从磁盘加载数据 而是使用init函数初始化
    pub async fn get_block_init<T: Copy>(
        &self,
        cid: CID,
        init: impl FnOnce(&mut [T]),
    ) -> Result<Arc<Cache>, SysError> {
        let mut blk = {
            let mut inner = self.inner.lock().await;
            debug_assert!(!inner.have_block_of(cid));
            inner.get_new_uninit_block()?
        };
        let sem = self.dirty_semaphore.take().await;
        init(blk.init_buffer()?);
        let mut inner = self.inner.lock().await;
        let c = inner.force_insert_block(blk, cid);
        inner.become_dirty(cid, &mut sem.into_multiply());
        Ok(c)
    }
    /// 从缓存块中释放块并取消同步任务
    pub async fn release_block(&self, cid: CID) {
        self.inner.lock().await.release_block(cid)
    }

    /// 生成一个同步任务
    pub fn sync_task(
        &mut self,
        concurrent: usize,
        mut spawn_fn: impl FnMut(Box<dyn Future<Output = ()> + Send + 'static>) + Send + 'static,
    ) -> impl Future<Output = ()> + Send + 'static {
        // 这一行保证了同步任务只会生成一次
        let init_inner = Arc::get_mut(&mut self.inner).unwrap().get_mut();
        let device = init_inner.device.clone();
        let data_sector_start = init_inner.data_sector_start;
        let spcl2 = init_inner.sector_per_cluster_log2;
        let sync = init_inner.sync_pending.clone();
        let manager = self.inner.clone();
        return async move {
            let sem = Arc::new(AtomicUsize::new(0));
            let waker = manager.lock().await.sync_waker();
            while let Ok(s) = WaitDirtyFuture(sync.clone()).await {
                for &cid in s.iter() {
                    let buffer = manager.lock().await.get_dirty_shared_buffer(cid).await;
                    WaitLessFuture(sem.as_ref(), concurrent).await;
                    sem.fetch_add(1, Ordering::Relaxed);
                    let device = device.clone();
                    let sem = sem.clone();
                    let waker = waker.clone();
                    let buffer = buffer.clone();
                    let sid = CacheManagerInner::raw_get_sid_of_cid(data_sector_start, spcl2, cid);
                    spawn_fn(Box::new(async move {
                        device.write_block(sid.0 as usize, &*buffer).await.unwrap();
                        sem.fetch_sub(1, Ordering::Relaxed);
                        waker.wake();
                    }));
                }
                manager.lock().await.dirty_suspend_iter(s.into_iter());
            }
        };
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
}
