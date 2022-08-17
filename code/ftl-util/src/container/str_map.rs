use alloc::{string::String, vec::Vec};

fn hash_fn(name: &str) -> usize {
    const INIT: usize = 97213;
    const MUL: usize = 95773;
    const ADD: usize = 85361;
    name.bytes().fold(INIT, |a, b| {
        (b as usize)
            .wrapping_mul(MUL)
            .wrapping_add(a)
            .wrapping_add(ADD)
    })
}

const MAX_TABLE: usize = 200;

pub struct StrMap<T> {
    table: [Vec<(String, T)>; MAX_TABLE],
    len: usize,
}

impl<T> StrMap<T> {
    const EMPTY_NODE: Vec<(String, T)> = Vec::new();
    pub const fn new() -> Self {
        Self {
            table: [Self::EMPTY_NODE; MAX_TABLE],
            len: 0,
        }
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn get(&self, name: &str) -> Option<&T> {
        let hash = hash_fn(name) % MAX_TABLE;
        for (k, v) in self.table[hash].iter() {
            if k == name {
                return Some(v);
            }
        }
        None
    }
    /// 如果存在重复则panic
    pub fn force_insert(&mut self, name: String, value: T) {
        let hash = hash_fn(&name) % MAX_TABLE;
        for (k, _) in self.table[hash].iter() {
            if k == &name {
                panic!()
            }
        }
        self.len += 1;
        self.table[hash].push((name, value));
    }
    pub fn force_remove(&mut self, name: &str) -> T {
        let hash = hash_fn(&name) % MAX_TABLE;
        for (i, (k, _)) in self.table[hash].iter().enumerate() {
            if k == name {
                self.len -= 1;
                return self.table[hash].remove(i).1;
            }
        }
        panic!()
    }
    pub fn iter<'a>(&'a self) -> impl Iterator<Item = (&'a String, &'a T)> {
        struct Iter<'a, T> {
            map: &'a StrMap<T>,
            index: (usize, usize),
        }
        impl<'a, T> Iterator for Iter<'a, T> {
            type Item = (&'a String, &'a T);

            fn next(&mut self) -> Option<Self::Item> {
                let (mut i, mut j) = self.index;
                while i < MAX_TABLE {
                    let node = &self.map.table[i][..];
                    if j < node.len() {
                        self.index = (i, j + 1);
                        return Some((&node[j].0, &node[j].1));
                    }
                    i += 1;
                    j = 0;
                }
                self.index = (MAX_TABLE, 0);
                None
            }
        }
        Iter {
            map: self,
            index: (0, 0),
        }
    }
}
