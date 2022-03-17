use alloc::boxed::Box;

use super::Stack;

/// 一个简单得不能再简单的单向链表
///
/// 为了提高效率，不提供Clone实现。
pub struct NeverCloneLinkedList<T> {
    node: Option<Box<Node<T>>>,
    size: usize,
}

struct Node<T> {
    data: T,
    next: Option<Box<Self>>,
}

impl<T> NeverCloneLinkedList<T> {
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
    pub fn clear(&mut self) {
        while let Some(_x) = self.pop() {}
    }
    pub fn retain(&mut self, mut f: impl FnMut(&T) -> bool) {
        unsafe {
            let mut node = &mut self.node as *mut Option<Box<Node<T>>>;
            while let Some(mut x) = (&mut *node).take() {
                if !f(&x.data) {
                    let Node { data, next } = *x;
                    drop(data);
                    *node = next;
                } else {
                    let next = &mut x.next as *mut _;
                    *node = Some(x);
                    node = next;
                }
            }
        }
    }
}

impl<T> const Default for NeverCloneLinkedList<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Stack<T> for NeverCloneLinkedList<T> {
    fn push(&mut self, data: T) {
        let next = self.node.take();
        self.size += 1;
        self.node = Some(Box::new(Node { data, next }))
    }
    fn pop(&mut self) -> Option<T> {
        let Node { data, next } = *self.node.take()?;
        self.size -= 1;
        self.node = next;
        Some(data)
    }
}

/// 递归改为循环，避免递归深度过大导致栈溢出
impl<T> Drop for NeverCloneLinkedList<T> {
    fn drop(&mut self) {
        self.clear();
    }
}
