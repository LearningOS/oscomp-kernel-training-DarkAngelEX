use core::marker::PhantomData;

use alloc::vec::Vec;

/// 用来向外发送元素的位置
pub trait IdxUpdater<V> {
    fn update(v: &mut V, new: usize);
    fn remove(v: &mut V);
}
pub struct NullUpdater;
impl<V> IdxUpdater<V> for NullUpdater {
    #[inline(always)]
    fn update(_v: &mut V, _new: usize) {}
    #[inline(always)]
    fn remove(_v: &mut V) {}
}
pub struct TraceUpdater;
impl IdxUpdater<*mut usize> for TraceUpdater {
    #[inline(always)]
    fn update(v: &mut *mut usize, new: usize) {
        unsafe { **v = new }
    }
    #[inline(always)]
    fn remove(v: &mut *mut usize) {
        unsafe { **v = usize::MAX }
    }
}

pub type TraceMaxHeap<T> = MaxHeapEx<T, *mut usize, TraceUpdater>;
unsafe impl<T: Ord + Send> Send for TraceMaxHeap<T> {}
unsafe impl<T: Ord + Sync> Sync for TraceMaxHeap<T> {}
/// 能够追踪元素位置的最大堆
///
/// data中每次元素的移动都会调用`IdxUpdater`中的更新函数
///
/// 此容器目前用来实现可以撤销的定时器
pub struct MaxHeapEx<T: Ord, V, F: IdxUpdater<V> = NullUpdater> {
    data: Vec<(T, V)>,
    _f: PhantomData<F>,
}

impl<T: Ord, V, F: IdxUpdater<V>> MaxHeapEx<T, V, F> {
    pub const fn new() -> Self {
        Self {
            data: Vec::new(),
            _f: PhantomData,
        }
    }
    pub fn len(&self) -> usize {
        self.data.len()
    }
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
    pub fn peek(&self) -> Option<&(T, V)> {
        self.data.first()
    }
    pub fn push(&mut self, v: (T, V)) {
        let old_len = self.len();
        self.data.push(v);
        self.sift_up(0, old_len);
    }
    pub fn pop(&mut self) -> Option<(T, V)> {
        let mut v = self.data.pop()?;
        if !self.data.is_empty() {
            unsafe { core::ptr::swap_nonoverlapping(&mut v, &mut self.data[0], 1) }
            F::update(&mut self.data[0].1, 0);
            self.sift_down_to_bottom(0);
        }
        F::remove(&mut v.1);
        Some(v)
    }
    /// 下标必须存在, 否则panic
    pub fn remove_idx(&mut self, idx: usize) -> (T, V) {
        debug_assert!(idx < self.len());
        let mut v = self.data.pop().unwrap();
        if idx == self.len() {
            F::remove(&mut v.1);
            return v;
        }
        unsafe { core::ptr::swap_nonoverlapping(&mut v, &mut self.data[idx], 1) }
        F::update(&mut self.data[idx].1, idx);
        if self.sift_up(0, idx) == idx {
            self.sift_down_to_bottom(idx);
        }
        F::remove(&mut v.1);
        v
    }
    /// 上浮节点, 如果父节点大于
    fn sift_up(&mut self, start: usize, mut pos: usize) -> usize {
        assert!(pos < self.data.len());
        while start < pos {
            let parent = get_parent(pos);
            if self.data[pos].0 <= self.data[parent].0 {
                break;
            }
            fast_swap(&mut self.data, pos, parent);
            F::update(&mut self.data[pos].1, pos);
            pos = parent;
        }
        F::update(&mut self.data[pos].1, pos);
        pos
    }
    fn sift_down_to_bottom(&mut self, mut pos: usize) {
        let len = self.len();
        loop {
            let child = get_child(pos);
            if child >= len {
                break;
            }
            // 只有一个孩子节点
            if child + 1 == len {
                if self.data[pos].0 < self.data[child].0 {
                    fast_swap(&mut self.data, pos, child);
                    F::update(&mut self.data[child].1, child);
                }
                break;
            }
            // 两个孩子中最大的下标
            let target = child + (self.data[child].0 < self.data[child + 1].0) as usize;
            if self.data[pos].0 >= self.data[target].0 {
                break;
            }
            fast_swap(&mut self.data, pos, target);
            F::update(&mut self.data[pos].1, pos);
            pos = target;
        }
        F::update(&mut self.data[pos].1, pos);
    }
    /// 出错就panic吧
    pub fn debug_check(&self, mut check: impl FnMut(usize, &V)) {
        for (i, (_, j)) in self.data.iter().enumerate() {
            check(i, j);
        }
    }
}

fn get_parent(pos: usize) -> usize {
    debug_assert!(pos != 0);
    (pos - 1) / 2
}

fn get_child(pos: usize) -> usize {
    pos * 2 + 1
}

/// 相对于`swap`, `swap_nonoverlapping`不会在栈上生成临时变量, 有更高的性能
///
/// 例: 交换两个`[usize; 3]`数组:
///
/// swap: 将两个数组复制到栈上, 再写到新的位置
/// swap_nonoverlapping: 遍历数组元素, 每次交换一个usize
fn fast_swap<T>(v: &mut [T], a: usize, b: usize) {
    debug_assert!(a != b);
    debug_assert!(a < v.len());
    debug_assert!(b < v.len());
    unsafe { core::ptr::swap_nonoverlapping(v.get_unchecked_mut(a), v.get_unchecked_mut(b), 1) }
}

#[test]
fn test() {
    use core::assert_matches::assert_matches;
    struct SetUpdater;
    impl IdxUpdater<usize> for SetUpdater {
        fn update(v: &mut usize, new: usize) {
            *v = new;
        }
        fn remove(_v: &mut usize) {}
    }
    let mut heap = MaxHeapEx::<_, _, SetUpdater>::new();

    heap.push((3, 0));
    heap.push((1, 0));
    heap.push((3, 0));
    heap.push((5, 0));
    heap.push((6, 0));
    heap.push((1, 0));
    heap.push((4, 0));
    heap.push((5, 0));
    heap.push((2, 0));
    heap.push((2, 0));
    heap.push((4, 0));
    heap.push((6, 0));
    for (i, &(_, j)) in heap.data.iter().enumerate() {
        assert_eq!(i, j);
    }
    assert_matches!(heap.pop(), Some((6, _)));
    assert_matches!(heap.pop(), Some((6, _)));
    assert_matches!(heap.pop(), Some((5, _)));
    assert_matches!(heap.pop(), Some((5, _)));
    assert_matches!(heap.pop(), Some((4, _)));
    assert_matches!(heap.pop(), Some((4, _)));
    assert_matches!(heap.pop(), Some((3, _)));
    assert_matches!(heap.pop(), Some((3, _)));
    assert_matches!(heap.pop(), Some((2, _)));
    assert_matches!(heap.pop(), Some((2, _)));
    assert_matches!(heap.pop(), Some((1, _)));
    assert_matches!(heap.pop(), Some((1, _)));
    assert_matches!(heap.pop(), None);

    heap.push((3, 0));
    heap.push((1, 0));
    heap.push((3, 0));
    heap.push((5, 0));
    heap.push((6, 0));
    heap.push((1, 0));
    heap.push((4, 0));
    heap.push((5, 0));
    heap.push((2, 0));
    heap.push((2, 0));
    heap.push((4, 0));
    heap.push((6, 0));

    heap.remove_idx(0);
    for (i, &(_, j)) in heap.data.iter().enumerate() {
        assert_eq!(i, j);
    }
    heap.remove_idx(5);
    for (i, &(_, j)) in heap.data.iter().enumerate() {
        assert_eq!(i, j);
    }
    heap.remove_idx(2);
    for (i, &(_, j)) in heap.data.iter().enumerate() {
        assert_eq!(i, j);
    }
    heap.remove_idx(1);
    for (i, &(_, j)) in heap.data.iter().enumerate() {
        assert_eq!(i, j);
    }
    heap.remove_idx(6);
    for (i, &(_, j)) in heap.data.iter().enumerate() {
        assert_eq!(i, j);
    }
    let mut min = usize::MAX;
    while let Some((v, _)) = heap.pop() {
        assert!(v <= min);
        min = v;
    }
}
