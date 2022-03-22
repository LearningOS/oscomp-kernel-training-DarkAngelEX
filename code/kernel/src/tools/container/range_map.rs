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
    pub fn try_insert(&mut self, Range { start, end }: Range<U>, value: V) -> Result<&mut V, V> {
        stack_trace!();
        assert!(start < end);
        if let Some((_, Node { end, .. })) = self.0.range(..end).next_back() {
            if *end > start {
                return Err(value);
            }
        }
        let node = self.0.try_insert(start, Node { end, value }).ok().unwrap();
        Ok(&mut node.value)
    }
    pub fn get(&self, key: U) -> Option<&V> {
        if let Some((_, Node { end, value })) = self.0.range(..=key).next_back() {
            if *end > key {
                return Some(value);
            }
        }
        None
    }
    pub fn force_remove_one(&mut self, Range { start, end }: Range<U>) -> V {
        stack_trace!();
        let Node { end: n_end, value } = self.0.remove(&start).unwrap();
        assert!(n_end == end);
        value
    }
    /// split_l: take the left side of the range
    ///
    /// split_r: take the right side of the range
    pub fn replace(
        &mut self,
        Range { start, end }: Range<U>,
        value: V,
        mut split_l: impl FnMut(&mut V, U) -> V,
        mut split_r: impl FnMut(&mut V, U) -> V,
        mut release: impl FnMut(V),
    ) {
        stack_trace!();
        if start >= end {
            return;
        }
        //  aaaaaaa  aaaaa
        //    bbb       bbbb
        //  aa---aa  aaa--
        if let Some((_, node)) = self.0.range_mut(..start).next_back() {
            if start < node.end {
                let mut v_m = split_r(&mut node.value, start);
                if end < node.end {
                    let v_r = split_r(&mut v_m, end);
                    release(v_m);
                    let value = Node {
                        end: node.end,
                        value: v_r,
                    };
                    self.0.try_insert(end, value).ok().unwrap();
                } else {
                    release(v_m);
                }
            }
        }
        //    aaaaaa
        //  bbbbb
        //    ---aaa
        if let Some((&key, node)) = self.0.range_mut(..end).next_back() {
            if end < node.end {
                release(split_l(&mut node.value, end));
                let node = self.0.remove(&key).unwrap();
                self.0.try_insert(end, node).ok().unwrap();
            }
        }
        //    aaa
        //  bbbbbbb
        //   - - -
        while let Some((&start, _node)) = self.0.range_mut(start..end).next() {
            release(self.0.remove(&start).unwrap().value);
        }
        self.0.try_insert(start, Node { end, value }).ok().unwrap();
    }
    pub fn clear(&mut self, mut release: impl FnMut(V)) {
        stack_trace!();
        while let Some((_start, node)) = self.0.pop_first() {
            release(node.value);
        }
    }
}
