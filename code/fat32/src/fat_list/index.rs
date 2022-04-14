use core::{cell::UnsafeCell, mem::MaybeUninit};

use alloc::{
    boxed::Box,
    sync::{Arc, Weak},
};

use crate::{mutex::spin_mutex::SpinMutex, xerror::SysError};

use super::unit::ListUnit;

/// 一个扇区缓存的索引
pub struct ListIndex {
    weak: Box<[UnsafeCell<Weak<ListUnit>>]>,
    lock: Box<[SpinMutex<()>]>,
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
            lock.fill_with(|| MaybeUninit::new(SpinMutex::new(())));
            self.weak = weak.assume_init();
            self.lock = lock.assume_init();
            Ok(())
        }
    }
    pub fn get(&self, index: usize) -> Option<Arc<ListUnit>> {
        let _lock = self.lock[index].lock();
        unsafe { (*self.weak[index].get()).upgrade() }
    }
    pub fn set(&self, index: usize, arc: &Arc<ListUnit>) {
        let _lock = self.lock[index].lock();
        unsafe { *self.weak[index].get() = Arc::downgrade(arc) }
    }
    pub fn reset(&self, index: usize) {
        let _lock = self.lock[index].lock();
        unsafe { *self.weak[index].get() = Weak::new() }
    }
}
