pub mod buffer;
pub mod manager;

use core::{
    ops::Deref,
    sync::atomic::{AtomicUsize, Ordering},
};

use alloc::{boxed::Box, sync::Arc};

use crate::{
    block_sync::SyncTask,
    layout::bpb::RawBPB,
    mutex::{ rw_sleep_mutex::RwSleepMutex},
    tools::{self, CID, SID},
    xerror::SysError,
    BlockDevice,
};

use self::buffer::Buffer;

pub enum CacheStatus {
    // 需要从磁盘读入数据
    None,
    // 使用Init函数加载数据, 而不是从磁盘读取数据
    Init(Box<dyn FnOnce(&mut [u8])>),
    // 和磁盘数据一致或已提交同步任务
    Clean,
    // 需要发送同步请求 只有引用计数为0才会同步
    Dirty,
}

impl CacheStatus {
    pub fn need_load(&self) -> bool {
        matches!(self, Self::None)
    }
    pub fn need_init(&self) -> bool {
        matches!(self, Self::Init(_))
    }
    pub fn can_read_now(&self) -> bool {
        match self {
            Self::None => false,
            Self::Init(_) => false,
            Self::Clean => true,
            Self::Dirty => true,
        }
    }
    /// take init_fn and leave Dirty
    pub fn take_init_fn(&mut self) -> Option<Box<dyn FnOnce(&mut [u8])>> {
        if !self.need_init() {
            return None;
        }
        if let Self::Init(f) = core::mem::replace(self, CacheStatus::Dirty) {
            return Some(f);
        }
        unreachable!()
    }
}

/// 此ID保证递增且不会到达上界
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct AccessID(pub usize);

impl AccessID {
    pub fn next(&mut self) -> Self {
        debug_assert_ne!(self.0, usize::MAX);
        let ret = *self;
        self.0 += 1;
        ret
    }
}

/// 缓存一个簇
///
/// 使用 S:MutexSupport 泛型参数是为了能在内核中关中断
pub struct Cache {
    cid: CID,
    access_id: AccessID,
    ref_count: AtomicUsize,
    inner: RwSleepMutex<CacheInner>,
}

impl Cache {
    pub fn new(buffer: Buffer) -> Self {
        Self {
            cid: CID(0),
            access_id: AccessID(0),
            ref_count: AtomicUsize::new(0),
            inner: RwSleepMutex::new(CacheInner::new(buffer)),
        }
    }
    pub fn cid(&self) -> CID {
        self.cid
    }
    pub fn init(&mut self, cid: CID, access_id: AccessID) {
        debug_assert!(self.no_owner());
        self.cid = cid;
        self.access_id = access_id;
    }
    pub fn set_init_fn(&mut self, init: impl FnOnce(&mut [u8]) + 'static) {
        assert!(self.no_owner());
        self.inner.get_mut().state = CacheStatus::Init(Box::new(init));
    }
    /// 以只读打开一个缓存块 允许多个进程同时访问
    pub async fn get_ro<T: Copy, V>(
        &self,
        op: impl FnOnce(&[T]) -> V,
        bpb: &RawBPB,
        device: &dyn BlockDevice,
    ) -> Result<V, SysError> {
        stack_trace!();
        if let Some(buffer) = self.inner.shared_lock().await.try_buffer_ro() {
            return Ok(op(buffer));
        }
        self.inner
            .unique_lock()
            .await
            .load_if_need(bpb.cid_transform(self.cid), device)
            .await?;
        Ok(op(self.inner.shared_lock().await.try_buffer_ro().unwrap()))
    }
    /// 以读写模式打开一个缓存块
    pub async fn get_rw<T: Copy, V>(
        &self,
        op: impl FnOnce(&mut [T]) -> V,
        bpb: &RawBPB,
        device: &dyn BlockDevice,
    ) -> Result<V, SysError> {
        stack_trace!();
        let mut lock = self.inner.unique_lock().await;
        if let Some(buffer) = lock.try_buffer_rw()? {
            return Ok(op(buffer));
        }
        lock.load_if_need(bpb.cid_transform(self.cid), device)
            .await?;
        Ok(op(lock.try_buffer_rw().unwrap().unwrap()))
    }
    /// 当op返回true时将调用apply函数
    pub async fn get_apply<T: Copy, V, A>(
        &self,
        op: impl FnOnce(&[T]) -> V,
        tran: impl FnOnce(&V) -> Option<A>,
        apply: impl FnOnce(A, &mut [T]),
        bpb: &RawBPB,
        device: &dyn BlockDevice,
    ) -> Result<V, SysError> {
        stack_trace!();
        let mut lock = self.inner.unique_lock().await;
        let buffer = match unsafe { lock.try_buffer_rw_no_dirty()? } {
            Some(buffer) => buffer,
            None => {
                let sid = bpb.cid_transform(self.cid);
                lock.load_if_need(sid, device).await?;
                unsafe { lock.try_buffer_rw_no_dirty().unwrap().unwrap() }
            }
        };
        let v = op(buffer);
        if let Some(a) = tran(&v) {
            apply(a, buffer);
            lock.set_dirty();
        }
        Ok(v)
    }
    /// 更新访问时间, 返回旧的值用于manager中更新顺序
    ///
    /// 需要确保在manager加锁状态中调用此函数 (唯一获取&mut Cache的方式)
    pub fn update_id(&mut self, new: AccessID) -> AccessID {
        core::mem::replace(&mut self.access_id, new)
    }
    /// 引用计数为0 非0时保证不回收
    pub fn no_owner(&self) -> bool {
        self.ref_count.load(Ordering::Relaxed) == 0
    }
    pub fn get_cache_ref(&self) -> CacheRef {
        self.ref_count.fetch_add(1, Ordering::Relaxed);
        CacheRef::new(self)
    }
}

/// 为了降低manager锁竞争 从manager中获取时不会分配内存与数据移动
///
/// 当处于磁盘读写状态时 如果写这个页则提供一个新副本
///
/// 当未处于磁盘读写状态时 直接取走这个页
pub struct CacheInner {
    state: CacheStatus,
    buffer: Buffer, // len == cluster
}

impl CacheInner {
    pub fn new(buffer: Buffer) -> Self {
        Self {
            state: CacheStatus::None,
            buffer,
        }
    }
    /// 如果数据未加载则返回None
    pub fn try_buffer_ro<T: Copy>(&self) -> Option<&[T]> {
        match &self.state {
            CacheStatus::None | CacheStatus::Init(_) => None,
            CacheStatus::Clean | CacheStatus::Dirty => Some(self.buffer.access_ro()),
        }
    }
    /// 如果数据未加载则从device加载数据
    pub fn try_buffer_rw<T: Copy>(&mut self) -> Result<Option<&mut [T]>, SysError> {
        if self.state.need_load() {
            return Ok(None);
        }
        let buffer = self.buffer.access_rw()?;
        if let Some(f) = self.state.take_init_fn() {
            f(tools::to_bytes_slice_mut(buffer));
        }
        self.state = CacheStatus::Dirty;
        Ok(Some(buffer))
    }
    /// 此函数和set_dirty配对
    pub unsafe fn try_buffer_rw_no_dirty<T: Copy>(&mut self) -> Result<Option<&mut [T]>, SysError> {
        if self.state.need_load() {
            return Ok(None);
        }
        let buffer = self.buffer.access_rw()?;
        if let Some(f) = self.state.take_init_fn() {
            f(tools::to_bytes_slice_mut(buffer));
        }
        Ok(Some(buffer))
    }
    pub fn set_dirty(&mut self) {
        debug_assert!(self.state.can_read_now());
        self.state = CacheStatus::Dirty;
    }
    pub async fn load_if_need(
        &mut self,
        sid: SID,
        device: &dyn BlockDevice,
    ) -> Result<(), SysError> {
        if self.state.can_read_now() {
            return Ok(());
        }
        stack_trace!();
        let buffer = self.buffer.access_rw()?;
        if let Some(init_fn) = self.state.take_init_fn() {
            init_fn(buffer);
            return Ok(());
        }
        device.read_block(sid.0 as usize, buffer).await?;
        self.state = CacheStatus::Clean;
        Ok(())
    }
    /// 阻塞在睡眠锁中
    pub async fn store_if_need(
        &mut self,
        sid: SID,
        device: &dyn BlockDevice,
    ) -> Result<(), SysError> {
        if !matches!(self.state, CacheStatus::Dirty) {
            return Ok(());
        }
        stack_trace!();
        let buffer = self.buffer.access_ro();
        device.write_block(sid.0 as usize, buffer).await?;
        self.state = CacheStatus::Clean;
        Ok(())
    }
    pub fn async_store_if_need(
        &mut self,
        sid: SID,
        device: &Arc<dyn BlockDevice>,
    ) -> Option<(SID, SyncTask)> {
        if !matches!(self.state, CacheStatus::Dirty) {
            return None;
        }
        let device = device.clone();
        let buffer = self.buffer.share();
        self.state = CacheStatus::Clean;
        Some((
            sid,
            SyncTask::new(async move {
                stack_trace!();
                device.write_block(sid.0 as usize, &*buffer).await
            }),
        ))
    }
}

pub struct CacheRef {
    cache: *const Cache,
}
impl CacheRef {
    pub fn new(cache: *const Cache) -> Self {
        Self { cache }
    }
}

impl Deref for CacheRef {
    type Target = Cache;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.cache }
    }
}

impl Drop for CacheRef {
    fn drop(&mut self) {
        unsafe {
            let prev = (*self.cache).ref_count.fetch_sub(1, Ordering::Relaxed);
            debug_assert_ne!(prev, 0);
        }
    }
}

impl Clone for CacheRef {
    fn clone(&self) -> Self {
        self.ref_count.fetch_add(1, Ordering::Relaxed);
        Self { cache: self.cache }
    }
}
