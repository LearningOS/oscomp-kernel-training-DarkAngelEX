use alloc::collections::BTreeSet;

use super::Stack;

pub struct PopSmallestSet<T: Ord> {
    set: BTreeSet<T>,
}

impl<T: Ord + Clone> Clone for PopSmallestSet<T> {
    fn clone(&self) -> Self {
        Self {
            set: self.set.clone(),
        }
    }
}

impl<T: Ord> PopSmallestSet<T> {
    pub const fn new() -> Self {
        Self {
            set: BTreeSet::new(),
        }
    }
    pub const fn len(&self) -> usize {
        self.set.len()
    }
    pub const fn is_empty(&self) -> bool {
        self.set.is_empty()
    }
    pub fn clear(&mut self) {
        self.set.clear()
    }
    pub fn retain(&mut self, f: impl FnMut(&T) -> bool) {
        self.set.retain(f)
    }
}

impl<T: Ord> Stack<T> for PopSmallestSet<T> {
    fn push(&mut self, data: T) {
        let f = self.set.insert(data);
        assert!(f);
    }
    fn pop(&mut self) -> Option<T> {
        self.set.pop_first()
    }
}
