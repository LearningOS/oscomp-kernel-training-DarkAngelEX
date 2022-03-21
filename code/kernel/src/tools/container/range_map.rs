use core::ops::Range;

use alloc::collections::BTreeMap;

struct Node<U, V> {
    pub end: U,
    pub value: V,
}

/// [start, end) -> value
///
/// 保证区间不重合 否则panic
pub struct RangeMap<U: Ord + Copy, V>(BTreeMap<U, Node<U, V>>);

impl<U: Ord + Copy, V> RangeMap<U, V> {
    pub const fn new() -> Self {
        Self(BTreeMap::new())
    }
    pub fn try_insert(&mut self, Range { start, end }: Range<U>, value: V) -> Result<(), V> {
        stack_trace!();
        assert!(start < end);
        if let Some((_, Node { end, .. })) = self.0.range(..end).next_back() {
            if *end > start {
                return Err(value);
            }
        }
        self.0.try_insert(start, Node { end, value }).ok().unwrap();
        Ok(())
    }
    pub fn get(&self, key: U) -> Option<&V> {
        if let Some((_, Node { end, value })) = self.0.range(..=key).next_back() {
            if *end > key {
                return Some(value);
            }
        }
        None
    }
    pub fn force_remove(&mut self, Range { start, end }: Range<U>) -> V {
        stack_trace!();
        let node = self.0.remove(&start).unwrap();
        assert!(node.end == end);
        node.value
    }
    pub fn replace(
        &mut self,
        Range { start, end }: Range<U>,
        value: V,
        mut clone: impl FnMut(&mut V) -> V,
        mut clear_range: impl FnMut(&mut V, Range<U>),
    ) {
        stack_trace!();
        if start >= end {
            return;
        }
        //  aaaaaaa  aaaaa
        //    bbb       bbbb
        //  aa   aa  aaa
        if let Some((_, node)) = self.0.range_mut(..start).next_back() {
            if node.end > start {
                clear_range(&mut node.value, start..end.min(node.end));
                if node.end > end {
                    let new_node = Node {
                        end: node.end,
                        value: clone(&mut node.value),
                    };
                    self.0.try_insert(end, new_node).ok().unwrap();
                } else {
                    node.end = start.min(node.end);
                }
            }
        }
        //    aaaaaa
        //  bbbbb
        //       aaa
        if let Some((&key, node)) = self.0.range_mut(..end).next_back() {
            if node.end > end {
                clear_range(&mut node.value, end..node.end);
                let node = self.0.remove(&key).unwrap();
                self.0.try_insert(end, node).ok().unwrap();
            }
        }
        //    aaa
        //  bbbbbbb
        //   - - -
        while let Some((&start, node)) = self.0.range_mut(start..end).next() {
            clear_range(&mut node.value, start..node.end);
            self.0.remove(&start).unwrap().value;
        }
        self.0.try_insert(start, Node { end, value }).ok().unwrap();
    }
    pub fn clear(&mut self, mut clear_range: impl FnMut(&mut V, Range<U>)) {
        stack_trace!();
        while let Some((start, mut node)) = self.0.pop_first() {
            clear_range(&mut node.value, start..node.end);
        }
    }
}
