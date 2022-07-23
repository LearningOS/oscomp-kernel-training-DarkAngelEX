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
    pub fn insert<R>(
        &self,
        node: &mut InListNode<T, A>,
        locked_run: impl FnOnce() -> R,
        release: impl FnOnce(&mut InListNode<T, A>),
    ) -> (R, Option<NonNull<InListNode<T, A>>>) {
        debug_assert!(node.is_empty());
        let mut lk = self.0.lock();
        lk.list.push_prev(node);
        let r = locked_run();
        if lk.cur < lk.max {
            lk.cur += 1;
            return (r, None);
        }
        let mut x = lk.list.pop_next().unwrap();
        unsafe {
            Some(release(x.as_mut()));
        }
        (r, Some(x))
    }
    pub fn remove_last(
        &self,
        release: impl FnOnce(&mut InListNode<T, A>),
    ) -> Option<NonNull<InListNode<T, A>>> {
        let mut lk = self.0.lock();
        let mut x = lk.list.pop_next()?;
        unsafe {
            Some(release(x.as_mut()));
        }
        Some(x)
    }
    pub fn try_remove(
        &self,
        node: &mut InListNode<T, A>,
        release: impl FnOnce(&mut InListNode<T, A>),
    ) -> Result<(), ()> {
        debug_assert!(!node.is_empty());
        let _lk = self.0.lock();
        if node.is_empty() {
            return Err(());
        }
        node.pop_self();
        release(node);
        Ok(())
    }
    /// 将node重新插入队尾
    pub fn update(&self, node: &mut InListNode<T, A>) {
        debug_assert!(!node.is_empty());
        let mut lk = self.0.lock();
        node.pop_self();
        lk.list.push_prev(node);
    }
    pub fn lock_run<R>(&self, f: impl FnOnce() -> R) -> R {
        let _lk = self.0.lock();
        f()
    }
}
