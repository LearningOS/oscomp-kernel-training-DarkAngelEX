use core::ptr::NonNull;

use crate::{
    list::InListNode,
    sync::{spin_mutex::SpinMutex, Spin},
};

/// LRU 管理器
///
/// 插入至头部, 移除队尾
pub struct LRUManager<T: 'static, A>(SpinMutex<Inner<T, A>, Spin>);

struct Inner<T, A> {
    list: InListNode<T, A>,
    cur: usize,
    max: usize,
}

impl<T: 'static, A: 'static> LRUManager<T, A> {
    pub fn new(max: usize) -> Self {
        Self(SpinMutex::new(Inner {
            list: InListNode::new(),
            cur: 0,
            max,
        }))
    }
    pub fn init(&mut self) {
        self.0.get_mut().list.init();
    }
    pub fn insert(
        &self,
        node: &mut InListNode<T, A>,
        release: impl FnOnce(&mut InListNode<T, A>),
    ) -> Option<NonNull<InListNode<T, A>>> {
        debug_assert!(node.is_empty());
        let mut lk = self.0.lock();
        lk.list.push_prev(node);
        if lk.cur < lk.max {
            lk.cur += 1;
            return None;
        }
        let mut x = lk.list.pop_next().unwrap();
        unsafe {
            Some(release(x.as_mut()));
        }
        Some(x)
    }
    pub fn remove(&self, node: &mut InListNode<T, A>) {
        debug_assert!(!node.is_empty());
        let _lk = self.0.lock();
        node.pop_self();
    }
    /// 将node重新插入队尾
    pub fn update(&self, node: &mut InListNode<T, A>) {
        debug_assert!(!node.is_empty());
        let mut lk = self.0.lock();
        node.pop_self();
        lk.list.push_prev(node);
    }
}
