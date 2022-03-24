use core::sync::atomic::{AtomicUsize, Ordering};

use alloc::{collections::BTreeMap, sync::Arc};

use crate::{
    memory::address::UserAddr4K,
    tools::{self, range::URange},
};

pub struct SharedCounter(Arc<AtomicUsize>);
impl Drop for SharedCounter {
    fn drop(&mut self) {
        panic!("SharedCounter must consume by SharedManager")
    }
}

impl SharedCounter {
    /// only available in this module
    fn consume(self) -> Arc<AtomicUsize> {
        unsafe { core::mem::transmute(self) }
    }
}

pub struct SCManager(BTreeMap<UserAddr4K, Arc<AtomicUsize>>);

impl Drop for SCManager {
    fn drop(&mut self) {
        assert!(self.0.is_empty());
    }
}

/// 管理共享页的引用计数, 原子计数实现
///
/// 此管理器的全部操作默认map中一定可以找到参数地址, 否则panic
impl SCManager {
    pub const fn new() -> Self {
        Self(BTreeMap::new())
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn insert(&mut self, ua: UserAddr4K) -> SharedCounter {
        let p = self
            .0
            .try_insert(ua, Arc::new(AtomicUsize::new(2)))
            .ok()
            .unwrap()
            .clone();
        SharedCounter(p)
    }
    pub fn insert_by(&mut self, ua: UserAddr4K, x: SharedCounter) {
        self.0.try_insert(ua, x.consume()).ok().unwrap();
    }
    pub fn clone_ua(&mut self, ua: UserAddr4K) -> SharedCounter {
        let x = self.0.get(&ua).unwrap().clone();
        x.fetch_add(1, Ordering::Relaxed);
        SharedCounter(x)
    }
    /// if map of ua is unique, return true
    pub fn remove_ua(&mut self, ua: UserAddr4K) -> bool {
        let x = self.0.remove(&ua).unwrap().fetch_sub(1, Ordering::Relaxed);
        debug_assert_ne!(x, 0);
        x == 1
    }
    /// if map of ua is unique, return Ok(())
    pub fn remove_ua_result(&mut self, ua: UserAddr4K) -> Result<(), ()> {
        tools::bool_result(self.remove_ua(ua))
    }
    /// 引用计数为 1 时释放并返回 true
    pub fn try_remove_unique(&mut self, ua: UserAddr4K) -> bool {
        self.0
            .get(&ua)
            .unwrap()
            .compare_exchange(1, 0, Ordering::Relaxed, Ordering::Relaxed)
            .map(|_| self.0.remove(&ua).unwrap())
            .is_ok()
    }
    /// 移除范围内存在的每一个计数器，并在计数器为1时调用release
    pub fn remove_release(&mut self, range: URange, mut release: impl FnMut(UserAddr4K)) {
        for (&addr, a) in self.0.range_mut(range.clone()) {
            if a.fetch_sub(1, Ordering::Relaxed) == 1 {
                release(addr);
            }
        }
        while let Some((&addr, _)) = self.0.range(range.clone()).next() {
            self.0.remove(&addr).unwrap();
        }
    }
    /// 禁止存在页面的引用计数为 1 否则panic
    ///
    /// 此函数只在错误回退时使用
    pub fn check_remove_all(&mut self) {
        self.0.iter().for_each(|(ua, sc)| {
            let a = sc.fetch_sub(1, Ordering::Relaxed);
            debug_assert!(a > 1, "a:{} > 1 ua:{:?}", a, ua);
        });
        self.0.clear();
    }
}
