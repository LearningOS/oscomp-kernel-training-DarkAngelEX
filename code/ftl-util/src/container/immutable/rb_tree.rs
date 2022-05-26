//! https://abhiroop.github.io/Haskell-Red-Black-Tree/
//!
//! 性能非常低的不可变红黑树 Arc产生大量原子指令
//!
//! 正常使用需要垃圾回收系统

use alloc::sync::Arc;
use core::cmp::Ordering::{Equal, Greater, Less};
use Color::{B, R};
use RB::{E, T};

pub struct RBTree<K: Ord, V: ?Sized>(Arc<RB<K, V>>);
impl<K: Ord, V: ?Sized> Clone for RBTree<K, V> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<K: Ord, V> RBTree<K, V> {
    pub fn insert(&self, key: K, value: V) -> Self {
        self.insert_arc(Arc::new((key, value)))
    }
}

impl<K: Ord, V: ?Sized> RBTree<K, V> {
    pub fn new() -> Self {
        Self(E.arc())
    }
    pub fn insert_arc(&self, kv: Arc<(K, V)>) -> Self {
        Self(insert(kv, &self.0).arc())
    }
    pub fn contain(&self, key: &K) -> bool {
        contain(key, &self.0)
    }
    pub fn search(&self, key: &K) -> Option<&V> {
        search(key, &self.0)
    }
    pub fn delete(&self, key: &K) -> Self {
        Self(delete(key, &self.0).arc())
    }
}

// #[test]
pub fn test() {
    let a = RBTree::new();
    let b = a.insert(1, 1).insert(2, 2).insert(3, 3);
    assert_eq!(b.contain(&1), true);
    assert_eq!(b.contain(&2), true);
    assert_eq!(b.contain(&3), true);
    assert_eq!(b.contain(&4), false);
    let c = b.delete(&2);
    assert_eq!(c.contain(&1), true);
    assert_eq!(c.contain(&2), false);
    assert_eq!(c.contain(&3), true);
}

#[derive(Clone)]
enum Color {
    R,
    B,
}
enum RB<K: Ord, V: ?Sized> {
    E,
    T(Color, Arc<Self>, Arc<(K, V)>, Arc<Self>),
}
impl<K: Ord, V: ?Sized> RB<K, V> {
    fn dup(&self) -> Self {
        Self::clone(self)
    }
    fn arc(self) -> Arc<Self> {
        Arc::new(self)
    }
}
impl<K: Ord, V: ?Sized> Clone for RB<K, V> {
    fn clone(&self) -> Self {
        match self {
            Self::E => Self::E,
            Self::T(a, b, c, d) => Self::T(a.clone(), b.clone(), c.clone(), d.clone()),
        }
    }
}

fn insert<K: Ord, V: ?Sized>(x: Arc<(K, V)>, s: &RB<K, V>) -> RB<K, V> {
    fn ins<K: Ord, V: ?Sized>(s: &RB<K, V>, x: Arc<(K, V)>) -> RB<K, V> {
        match s {
            E => T(R, E.arc(), x, E.arc()),
            T(B, a, y, b) => match x.0.cmp(&y.0) {
                Less => balance(ins(a, x), y.clone(), b.dup()),
                Greater => balance(a.dup(), y.clone(), ins(b, x)),
                Equal => s.dup(),
            },
            T(R, a, y, b) => match x.0.cmp(&y.0) {
                Less => T(R, ins(a, x).arc(), y.clone(), b.clone()),
                Greater => T(R, a.clone(), y.clone(), ins(b, x).arc()),
                Equal => s.dup(),
            },
        }
    }
    match ins(s, x) {
        T(_, a, z, b) => return T(B, a.clone(), z.clone(), b.clone()),
        E => unreachable!(),
    }
}

fn contain<K: Ord, V: ?Sized>(x: &K, s: &RB<K, V>) -> bool {
    match s {
        E => false,
        T(_, a, y, b) => match x.cmp(&y.0) {
            Equal => true,
            Less => contain(x, a),
            Greater => contain(x, b),
        },
    }
}

fn search<'a, K: Ord, V: ?Sized>(x: &K, s: &'a RB<K, V>) -> Option<&'a V> {
    match s {
        E => None,
        T(_, a, y, b) => match x.cmp(&y.0) {
            Equal => Some(&y.1),
            Less => search(x, a),
            Greater => search(x, b),
        },
    }
}

fn balance<K: Ord, V: ?Sized>(a: RB<K, V>, x: Arc<(K, V)>, b: RB<K, V>) -> RB<K, V> {
    match (a, x, b) {
        (T(R, a, x, b), y, T(R, c, z, d)) => T(R, T(B, a, x, b).arc(), y, T(B, c, z, d).arc()),
        (T(R, t, y, c), z, d) if let T(R, a, x, b) = t.dup() => T(R, T(B, a, x, b).arc(), y, T(B, c, z, d.arc()).arc()),
        (T(R, a, x, t),z,d)if let T(R, b, y, c) = t.dup() => T(R, T(B, a, x, b).arc(), y, T(B, c, z, d.arc()).arc()),
        (a, x, T(R, b, y, t)) if let T(R, c, z ,d) = t.dup() => T(R, T(B, a.arc(), x, b).arc(), y, T(B, c, z, d).arc()),
        (a, x, T(R, t, z , d)) if let T(R, b, y ,c) = t.dup() => T(R, T(B, a.arc(), x, b).arc(), y, T(B, c, z, d).arc()),
        (a, x, b) => T(B, a.arc(), x, b.arc()),
    }
}

fn delete<K: Ord, V: ?Sized>(x: &K, t: &RB<K, V>) -> RB<K, V> {
    return match del(t, x) {
        T(_, a, y, b) => T(B, a, y, b),
        E => E,
    };
    fn del<K: Ord, V: ?Sized>(t: &RB<K, V>, x: &K) -> RB<K, V> {
        match t {
            E => E,
            T(_, a, y, b) => match x.cmp(&y.0) {
                Less => delform_left(a, y.clone(), b.dup(), x),
                Greater => delform_right(a.dup(), y.clone(), b, x),
                Equal => app(a.dup(), b.dup()),
            },
        }
    }
    fn delform_left<K: Ord, V: ?Sized>(
        a: &RB<K, V>,
        y: Arc<(K, V)>,
        b: RB<K, V>,
        x: &K,
    ) -> RB<K, V> {
        match a {
            T(B, _, _, _) => balleft(del(a, x), y, b),
            _ => T(R, del(a, x).arc(), y, b.arc()),
        }
    }
    fn delform_right<K: Ord, V: ?Sized>(
        a: RB<K, V>,
        y: Arc<(K, V)>,
        b: &RB<K, V>,
        x: &K,
    ) -> RB<K, V> {
        match b {
            T(B, _, _, _) => balright(a, y, del(b, x)),
            _ => T(R, a.arc(), y, del(b, x).arc()),
        }
    }
    fn balleft<K: Ord, V: ?Sized>(tl: RB<K, V>, x: Arc<(K, V)>, tr: RB<K, V>) -> RB<K, V> {
        match (tl, x, tr) {
            (T(R, a, x, b), y, c) => T(R, T(B, a, x, b).arc(), y, c.arc()),
            (bl, x, T(B, a, y, b)) => balance(bl, x, T(R, a, y, b)),
            (bl, x, T(R, t, z, c)) if let T(B,a, y,b) = t.dup() => {
                T(R, T(B, bl.arc(), x, a).arc(), y, balance(b.dup(), z, sub1(c.dup())).arc())
            }
            _ => unreachable!(),
        }
    }
    fn balright<K: Ord, V: ?Sized>(tl: RB<K, V>, x: Arc<(K, V)>, tr: RB<K, V>) -> RB<K, V> {
        match (tl, x, tr) {
            (a, x, T(R, b, y, c)) => T(R, a.arc(), x, T(B, b, y, c).arc()),
            (T(B, a, x, b), y, bl) => balance(T(R, a, x, b), y, bl),
            (T(R, a, x, t),z,bl) if let T(B, b, y, c) = t.dup() => {
                T(R, balance(sub1(a.dup()), x, b.dup()).arc(), y, T(B, c, z, bl.arc()).arc())
            },
            _ => unreachable!(),
        }
    }
    fn sub1<K: Ord, V: ?Sized>(t: RB<K, V>) -> RB<K, V> {
        match t {
            T(B, a, x, b) => T(R, a, x, b),
            _ => unreachable!(),
        }
    }
    fn app<K: Ord, V: ?Sized>(a: RB<K, V>, b: RB<K, V>) -> RB<K, V> {
        match (a, b) {
            (E, x) | (x, E) => x,
            (T(R, a, x, b), T(R, c, y, d)) => match app(b.dup(), c.dup()) {
                T(R, b, z, c) => T(R, T(R, a, x, b).arc(), z, T(R, c, y, d).arc()),
                bc => T(R, a, x, T(R, bc.arc(), y, d).arc()),
            },
            (T(B, a, x, b), T(B, c, y, d)) => match app(b.dup(), c.dup()) {
                T(R, b, z, c) => T(R, T(B, a, x, b).arc(), z, T(B, c, y, d).arc()),
                bc => balleft(a.dup(), x, T(B, bc.arc(), y, d)),
            },
            (a, T(R, b, x, c)) => T(R, app(a, b.dup()).arc(), x, c),
            (T(R, a, x, b), c) => T(R, a, x, app(b.dup(), c).arc()),
        }
    }
}
