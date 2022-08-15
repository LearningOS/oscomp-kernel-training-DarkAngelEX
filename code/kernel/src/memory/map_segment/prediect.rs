use alloc::{collections::VecDeque, vec::Vec};

use crate::{memory::address::UserAddr4K, sync::mutex::SpinLock};

const TARGET: usize = 10;

/// 缺页错误预测器, 预测前TARGET个缺页异常
pub struct Predicter {
    inner: SpinLock<Inner>,
}

struct Inner {
    fifo: VecDeque<UserAddr4K>,
    cnt: usize, // 只接收前TARGET个结果
}

impl Predicter {
    pub fn new() -> Self {
        Self {
            inner: SpinLock::new(Inner {
                fifo: VecDeque::new(),
                cnt: 0,
            }),
        }
    }
    pub fn insert(&self, ua: UserAddr4K) {
        if unsafe { self.inner.unsafe_get().cnt >= TARGET } {
            return;
        }
        let mut lk = self.inner.lock();
        if lk.cnt >= TARGET {
            return;
        }
        if lk.fifo.contains(&ua) {
            return;
        }
        lk.fifo.push_back(ua);
        lk.cnt += 1;
        if lk.fifo.len() <= TARGET {
            return;
        }
        lk.fifo.pop_front().unwrap();
    }
    /// 返回有序集合
    pub fn take_in_order(&self) -> Vec<UserAddr4K> {
        let mut lk = self.inner.lock();
        lk.cnt = 0;
        let mut v: Vec<_> = lk.fifo.iter().copied().collect();
        v.sort();
        v
    }
}
