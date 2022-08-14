use alloc::{
    collections::{BTreeSet, VecDeque},
    vec::Vec,
};

use crate::{memory::address::UserAddr4K, sync::mutex::SpinLock};

const TARGET: usize = 10;

/// 缺页错误预测器
pub struct Predicter {
    inner: SpinLock<Inner>,
}

struct Inner {
    fifo: VecDeque<UserAddr4K>,
    set: BTreeSet<UserAddr4K>, // 去重
}

impl Predicter {
    pub fn new() -> Self {
        Self {
            inner: SpinLock::new(Inner {
                fifo: VecDeque::new(),
                set: BTreeSet::new(),
            }),
        }
    }
    pub fn insert(&self, ua: UserAddr4K) {
        let mut lk = self.inner.lock();
        if !lk.set.insert(ua) {
            return;
        }
        lk.fifo.push_back(ua);
        if lk.fifo.len() <= TARGET {
            return;
        }
        let old = lk.fifo.pop_front().unwrap();
        let r = lk.set.remove(&old);
        debug_assert!(r);
    }
    /// 有序的迭代器
    pub fn take_in_order(&self) -> Vec<UserAddr4K> {
        self.inner.lock().set.iter().map(|a| *a).collect()
    }
}
