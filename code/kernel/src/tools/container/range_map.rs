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
        let (_, Node { end, value }) = self.0.range(..=key).next_back()?;
        if *end > key {
            return Some(value);
        }
        None
    }
    /// range 处于返回值对应的 range 内
    pub fn range_contain(&self, range: Range<U>) -> Option<&V> {
        let (_, Node { end, value }) = self.0.range(..=range.start).next_back()?;
        if *end >= range.end {
            return Some(value);
        }
        None
    }
    /// range 完全匹配返回值所在范围
    pub fn range_match(&self, range: Range<U>) -> Option<&V> {
        let (start, Node { end, value }) = self.0.range(..=range.start).next_back()?;
        if *start == range.start && *end == range.end {
            return Some(value);
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
    pub fn remove(
        &mut self,
        Range { start, end }: Range<U>,
        mut split_l: impl FnMut(&mut V, U, Range<U>) -> V,
        mut split_r: impl FnMut(&mut V, U, Range<U>) -> V,
        mut release: impl FnMut(V, Range<U>),
    ) {
        stack_trace!();
        if start >= end {
            return;
        }
        //  aaaaaaa  aaaaa
        //    bbb       bbbb
        //  aa---aa  aaa--
        if let Some((&n_start, node)) = self.0.range_mut(..start).next_back() {
            if start < node.end {
                let mut v_m = split_r(&mut node.value, start, n_start..node.end);
                if end < node.end {
                    let v_r = split_r(&mut v_m, end, start..node.end);
                    release(v_m, start..end);
                    let value = Node {
                        end: node.end,
                        value: v_r,
                    };
                    self.0.try_insert(end, value).ok().unwrap();
                } else {
                    release(v_m, start..node.end);
                }
            }
        }
        //    aaaaaa
        //  bbbbb
        //    ---aaa
        if let Some((&n_start, node)) = self.0.range_mut(..end).next_back() {
            if end < node.end {
                release(
                    split_l(&mut node.value, end, n_start..node.end),
                    n_start..end,
                );
                let node = self.0.remove(&n_start).unwrap();
                self.0.try_insert(end, node).ok().unwrap();
            }
        }
        //    aaa
        //  bbbbbbb
        //    ---
        while let Some((&n_start, node)) = self.0.range(start..end).next() {
            let n_end = node.end;
            release(self.0.remove(&n_start).unwrap().value, n_start..n_end);
        }
    }
    pub fn replace(
        &mut self,
        r @ Range { start, end }: Range<U>,
        value: V,
        split_l: impl FnMut(&mut V, U, Range<U>) -> V,
        split_r: impl FnMut(&mut V, U, Range<U>) -> V,
        release: impl FnMut(V, Range<U>),
    ) {
        self.remove(r, split_l, split_r, release);
        self.0.try_insert(start, Node { end, value }).ok().unwrap();
    }
    pub fn clear(&mut self, mut release: impl FnMut(V, Range<U>)) {
        stack_trace!();
        core::mem::take(&mut self.0)
            .into_iter()
            .for_each(|(n_start, node)| release(node.value, n_start..node.end));
    }
    pub fn retain(&mut self, mut f: impl FnMut(&mut V, Range<U>) -> bool) {
        self.0.retain(|&a, b| f(&mut b.value, a..b.end))
    }
    /// f return (A, B)
    ///
    /// if A is Some will set current into A, else do nothing.
    ///
    /// B will insert to new range_map.
    pub fn fork(&mut self, mut f: impl FnMut(&V) -> V) -> Self {
        // use crate::tools::container::{never_clone_linked_list::NeverCloneLinkedList, Stack};
        let mut map = RangeMap::new();
        for (&a, Node { end: b, value: v }) in self.0.iter() {
            let node = Node {
                end: *b,
                value: f(v),
            };
            map.0.try_insert(a, node).ok().unwrap();
        }
        map
    }
    /// 必须存在 range 对应的 node
    pub fn merge(&mut self, Range { start, end }: Range<U>, mut f: impl FnMut(&V, &V) -> bool) {
        let cur = self.0.get(&start).unwrap();
        assert!(cur.end == end);
        let cur = if let Some(nxt) = self.0.get(&end)
         && f(&cur.value, &nxt.value) {
            let nxt_end = nxt.end;
            self.0.remove(&end).unwrap();
            let cur = self.0.get_mut(&start).unwrap();
            cur.end = nxt_end;
            // self.0.get(&start).unwrap() // unnecessary
            unsafe { &*(&*cur as *const _) }
        } else {
            cur
        };
        if let Some((&s, n)) = self.0.range(..start).next_back() {
            if n.end == start && f(&n.value, &cur.value) {
                self.0.get_mut(&s).unwrap().end = cur.end;
                self.0.remove(&start).unwrap();
            }
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = (Range<U>, &V)> {
        self.0.iter().map(|(&s, n)| {
            let r = Range {
                start: s,
                end: n.end,
            };
            (r, &n.value)
        })
    }
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (Range<U>, &mut V)> {
        self.0.iter_mut().map(|(&s, n)| {
            let r = Range {
                start: s,
                end: n.end,
            };
            (r, &mut n.value)
        })
    }
}
