//! FAT链表扇区缓存块索引器
//!
//! 索引器自身无锁, 被inner更新
use core::{cell::UnsafeCell, mem::MaybeUninit};

use alloc::{
    boxed::Box,
    sync::{Arc, Weak},
};
use ftl_util::error::SysError;

use crate::mutex::rw_spin_mutex::RwSpinMutex;

use super::unit::ListUnit;

/// 一个扇区缓存的索引
///
/// 不使用任何异步操作
pub(crate) struct ListIndex {
    weak: Box<[UnsafeCell<Weak<ListUnit>>]>,
    lock: Box<[RwSpinMutex<()>]>,
}

unsafe impl Send for ListIndex {}
unsafe impl Sync for ListIndex {}

impl ListIndex {
    pub fn new() -> Self {
        Self {
            weak: Box::new([]),
            lock: Box::new([]),
        }
    }
    pub fn init(&mut self, size: usize) -> Result<(), SysError> {
        assert!(self.weak.is_empty());
        assert!(self.lock.is_empty());
        let mut weak = Box::try_new_uninit_slice(size)?;
        let mut lock = Box::try_new_uninit_slice(size)?;
        unsafe {
            weak.fill_with(|| MaybeUninit::new(UnsafeCell::new(Weak::<ListUnit>::new())));
            lock.fill_with(|| MaybeUninit::new(RwSpinMutex::new(())));
            self.weak = weak.assume_init();
            self.lock = lock.assume_init();
            Ok(())
        }
    }
    pub fn get(&self, index: usize) -> Option<Arc<ListUnit>> {
        let _lock = self.lock[index].shared_lock();
        unsafe { (*self.weak[index].get()).upgrade() }
    }
    pub fn set(&self, index: usize, arc: &Arc<ListUnit>) {
        let _lock = self.lock[index].unique_lock();
        unsafe { *self.weak[index].get() = Arc::downgrade(arc) }
    }
    // pub fn reset(&self, index: usize) {
    //     let _lock = self.lock[index].unique_lock();
    //     unsafe { *self.weak[index].get() = Weak::new() }
    // }
}
