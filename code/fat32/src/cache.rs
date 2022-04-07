use core::cell::UnsafeCell;

use alloc::sync::Arc;

use crate::tools::CID;

pub enum CacheStatus {
    Common,   // 数据有效
    NeedLoad, // 需要从磁盘读入数据
    NeedCopy, // 正在同步至磁盘 写缓存块需要产生新副本
}

pub struct Cache {
    inner: CacheInner,
}

/// 为了降低manager锁竞争 从manager中获取时不会分配内存与数据移动
///
/// 当处于磁盘读写状态时 如果写这个页则提供一个新副本
///
/// 当未处于磁盘读写状态时 直接取走这个页
pub struct CacheInner {
    cid: CID,
    state: CacheStatus,
    buffer: Option<Arc<UnsafeCell<[u8]>>>, // len == cluster
}

pub struct CacheManager {}
