//! FAT链表扇区缓存块索引器
//!
//! 索引器自身无锁, 被inner更新
use core::mem::MaybeUninit;

use alloc::{
    boxed::Box,
    sync::{Arc, Weak},
};
use ftl_util::{error::SysR, rcu::RcuWraper};

use super::unit::ListUnit;

/// 一个扇区缓存的索引
///
/// 不使用任何异步操作
pub(crate) struct ListIndex {
    weak: Box<[RcuWraper<Weak<ListUnit>>]>,
}

impl ListIndex {
    pub fn new() -> Self {
        Self { weak: Box::new([]) }
    }
    pub fn init(&mut self, size: usize) -> SysR<()> {
        assert!(self.weak.is_empty());
        let mut weak = Box::try_new_uninit_slice(size)?;
        unsafe {
            weak.fill_with(|| MaybeUninit::new(RcuWraper::new(Weak::<ListUnit>::new())));
            self.weak = weak.assume_init();
            Ok(())
        }
    }
    pub fn get(&self, index: usize) -> Option<Arc<ListUnit>> {
        self.weak[index].rcu_read().upgrade()
    }
    pub fn set(&self, index: usize, arc: &Arc<ListUnit>) {
        self.weak[index].rcu_write_atomic(Arc::downgrade(arc));
    }
}
