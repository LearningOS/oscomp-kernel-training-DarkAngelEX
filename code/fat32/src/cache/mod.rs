pub mod buffer;
pub mod manager;

use core::{
    cell::UnsafeCell,
    ops::Deref,
    sync::atomic::{AtomicUsize, Ordering},
};

use crate::{mutex::MutexSupport, sleep_mutex::RwSleepMutex, tools::CID};

use self::buffer::Buffer;

pub enum CacheStatus {
    None,  // 需要从磁盘读入数据
    Clean, // 和磁盘数据一致或已提交同步任务
    Dirty, // 需要同步
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
pub struct Cache<S: MutexSupport> {
    cid: CID,
    access_id: AccessID,
    ref_count: AtomicUsize,
    inner: RwSleepMutex<CacheInner, S>,
}

impl<S: MutexSupport> Cache<S> {
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
    /// 以只读打开一个缓存块 允许多个进程同时进行
    pub async fn get_ro(&self, _f: impl FnOnce(&[u8]) -> Result<(), ()>) -> Result<(), ()> {
        todo!()
    }
    /// 以读写模式打开一个缓存块
    pub async fn get_rw(&self, _f: impl FnOnce(&mut [u8]) -> Result<(), ()>) -> Result<(), ()> {
        todo!()
    }
    /// 使用此函数获取的值为无效值 缓存块不存在也不会向磁盘发送申请
    ///
    /// debug模式将初始化为0
    pub async fn get_create(&self, _f: impl FnOnce(&mut [u8]) -> Result<(), ()>) -> Result<(), ()> {
        todo!()
    }
    /// 更新访问时间, 返回旧的值用于manager中更新顺序
    ///
    /// 需要确保在manager加锁状态中调用此函数
    pub unsafe fn update_id(&mut self, new: AccessID) -> AccessID {
        core::mem::replace(&mut self.access_id, new)
    }
    /// 引用计数为0 非0时保证不回收
    pub fn no_owner(&self) -> bool {
        self.ref_count.load(Ordering::Relaxed) == 0
    }
    pub fn get_cache_ref(&self) -> CacheRef<S> {
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
}

pub struct CacheRef<S: MutexSupport> {
    cache: *const Cache<S>,
}
impl<S: MutexSupport> CacheRef<S> {
    pub fn new(cache: *const Cache<S>) -> Self {
        Self { cache }
    }
}

impl<S: MutexSupport> Deref for CacheRef<S> {
    type Target = Cache<S>;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.cache }
    }
}

impl<S: MutexSupport> Drop for CacheRef<S> {
    fn drop(&mut self) {
        unsafe {
            let prev = (*self.cache).ref_count.fetch_sub(1, Ordering::Relaxed);
            debug_assert_ne!(prev, 0);
        }
    }
}

impl<S: MutexSupport> Clone for CacheRef<S> {
    fn clone(&self) -> Self {
        self.ref_count.fetch_add(1, Ordering::Relaxed);
        Self { cache: self.cache }
    }
}
