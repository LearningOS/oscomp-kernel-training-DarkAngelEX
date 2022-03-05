use alloc::sync::Arc;

use super::Stack;

/// 可持久化单向链表
///
/// Clone时间复杂度为O(1)，但增加了Arc开销。
#[derive(Clone)]
pub struct FastCloneLinkedList<T: Copy> {
    node: Option<Arc<Node<T>>>,
    size: usize,
}

struct Node<T: Copy> {
    data: T,
    next: Option<Arc<Self>>,
}

impl<T: Copy> FastCloneLinkedList<T> {
    pub const fn new() -> Self {
        Self {
            node: None,
            size: 0,
        }
    }
    pub const fn len(&self) -> usize {
        self.size
    }
    pub const fn is_empty(&self) -> bool {
        self.node.is_none()
    }
    /// 递归改为循环，避免递归深度过大导致栈溢出
    pub fn clear(&mut self) {
        let mut node = self.node.take();
        while let Some(x) = node {
            node = x.next.clone();
        }
        self.size = 0;
    }
}
impl<T: Copy> Stack<T> for FastCloneLinkedList<T> {
    fn push(&mut self, data: T) {
        let next = self.node.take();
        self.size += 1;
        self.node = Some(Arc::new(Node { data, next }))
    }
    /// attention! data will be cloned from Arc.
    fn pop(&mut self) -> Option<T> {
        let this = self.node.take()?;
        self.size -= 1;
        self.node = this.next.clone();
        Some(this.data)
    }
}

impl<T: Copy> Default for FastCloneLinkedList<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Copy> Drop for FastCloneLinkedList<T> {
    fn drop(&mut self) {
        self.clear();
    }
}
