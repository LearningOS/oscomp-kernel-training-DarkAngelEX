use core::ptr::NonNull;

use alloc::boxed::Box;

use super::{Sig, SignalSet, SIG_N};

const RT_N: usize = SIG_N - 32;
/// sig_union 中低 SIG_MAXBIT bit 用来放置信号ID
const SIG_MAXBIT: usize = 16;

struct Node {
    prev: Option<NonNull<Node>>,
    next: Option<NonNull<Node>>,
    sig_next: Option<NonNull<Node>>,
    sig_union: u64, // [48|16] => [access ID | SIG number]
}

impl Node {
    pub fn new(sig_union: u64) -> NonNull<Self> {
        let node = Box::new(Node {
            prev: None,
            next: None,
            sig_next: None,
            sig_union,
        });
        NonNull::new(Box::into_raw(node)).unwrap()
    }
    pub unsafe fn free(ptr: NonNull<Self>) {
        drop(Box::from_raw(ptr.as_ptr()))
    }
    pub fn insert_after(last: NonNull<Self>, node: NonNull<Self>) {
        unsafe {
            debug_assert!((*last.as_ptr()).next.is_none());
            (*last.as_ptr()).next = Some(node);
            (*node.as_ptr()).prev = Some(last);
        }
    }
    pub fn insert_after_unit(last: NonNull<Self>, node: NonNull<Self>) {
        unsafe {
            debug_assert!((*last.as_ptr()).sig_next.is_none());
            debug_assert!((*node.as_ptr()).sig_next.is_none());
            (*last.as_ptr()).sig_next = Some(node);
        }
    }
    pub fn remove(this: NonNull<Self>) {
        unsafe {
            if let Some(prev) = (*this.as_ptr()).prev {
                (*prev.as_ptr()).next = None;
            }
            if let Some(next) = (*this.as_ptr()).next {
                (*next.as_ptr()).prev = None;
            }
        }
    }
    pub fn sig(&self) -> Sig {
        Sig((self.sig_union as u32) & ((1 << SIG_MAXBIT) - 1))
    }
}

/// 实时信号管理器
///
/// 提供O(1)的插入与有掩码的按序取出操作
pub struct RTQueue {
    head: Option<NonNull<Node>>,
    tail: Option<NonNull<Node>>,
    access: u64,
    exist: SignalSet,
    table: [(Option<NonNull<Node>>, Option<NonNull<Node>>); RT_N], // head, tail
}

unsafe impl Send for RTQueue {}
unsafe impl Sync for RTQueue {}

impl Drop for RTQueue {
    fn drop(&mut self) {
        let mut cur = self.head;
        while let Some(p) = cur {
            unsafe {
                cur = (*p.as_ptr()).next;
                Node::free(p);
            }
        }
    }
}

impl RTQueue {
    #[inline]
    pub fn new() -> Self {
        Self {
            head: None,
            tail: None,
            access: 0,
            exist: SignalSet::EMPTY,
            table: [(None, None); _],
        }
    }
    fn alloc_access(&mut self, sig: Sig) -> u64 {
        let sig_mask = 1u64 << SIG_MAXBIT;
        debug_assert!(sig.0 < sig_mask as u32);
        debug_assert!(self.access & (sig_mask - 1) == 0);
        let out = self.access | sig.0 as u64;
        self.access += sig_mask;
        out
    }
    /// O(1)插入信号
    pub fn receive(&mut self, sig: Sig) {
        stack_trace!();
        sig.check();
        let node = Node::new(self.alloc_access(sig));
        // 插入队列
        match self.tail {
            Some(last) => {
                debug_assert!(self.head.is_some());
                Node::insert_after(last, node);
            }
            None => {
                debug_assert!(self.head.is_none());
                self.head = Some(node);
            }
        }
        self.tail = Some(node);
        // 插入信号单元队列
        let unit = &mut self.table[sig.0 as usize - 32];
        match unit.1 {
            Some(last) => {
                debug_assert!(unit.0.is_some());
                Node::insert_after_unit(last, node);
                unit.1 = Some(node);
            }
            None => {
                debug_assert!(unit.0.is_none());
                *unit = (Some(node), Some(node));
                self.exist.insert_bit(sig);
            }
        }
    }
    /// 此操作不需要锁
    pub fn can_fetch(&self, mask: &SignalSet) -> bool {
        self.exist.can_fetch(mask)
    }
    unsafe fn remove_node_unit(&mut self, sig: Sig, node: NonNull<Node>) {
        let unit = &mut self.table[sig.0 as usize - 32];
        debug_assert!(Some(node) == unit.0);
        let next = (*node.as_ptr()).sig_next;
        unit.0 = next;
        if next.is_none() {
            unit.1 = None;
            self.exist.remove_bit(sig);
        }
    }
    unsafe fn remove_node_main(&mut self, node: NonNull<Node>) {
        let n_prev = node.as_ref().prev;
        let n_next = node.as_ref().next;
        match n_prev {
            Some(prev) => {
                debug_assert!((*prev.as_ptr()).next == Some(node));
                (*prev.as_ptr()).next = n_next;
            }
            None => {
                debug_assert!(self.head == Some(node));
                self.head = n_next;
            }
        }
        match n_next {
            Some(next) => {
                debug_assert!((*next.as_ptr()).prev == Some(node));
                (*next.as_ptr()).prev = n_prev;
            }
            None => {
                debug_assert!(self.tail == Some(node));
                self.tail = n_prev;
            }
        }
    }
    /// must remove first signal
    fn remove_node(&mut self, sig: Sig, node: NonNull<Node>) {
        unsafe {
            self.remove_node_unit(sig, node);
            self.remove_node_main(node);
        }
    }
    /// O(1)取出最早的不被阻塞的信号ID
    pub fn fetch(&mut self, mask: &SignalSet) -> Option<Sig> {
        const DIRECT_FETCH: usize = 8;
        if !self.can_fetch(mask) {
            return None;
        }
        let mut cur = self.head.unwrap();
        for _ in 0..DIRECT_FETCH {
            unsafe {
                let sig = (*cur.as_ptr()).sig();
                if mask.get_bit(sig) == false {
                    self.remove_node(sig, cur);
                    Node::free(cur);
                    return Some(sig);
                }
                cur = (*cur.as_ptr()).next?;
            }
        }
        // 已经通过了can_fetch判断，一定存在可以发射的信号
        let mut set = self.exist;
        set.remove(mask);
        let ans = set
            .bit_fold(None, |sig, acc: Option<u64>| unsafe {
                let this = (*self.table[sig as usize - 32].0.unwrap().as_ptr()).sig_union;
                let ret = match acc {
                    None => this,
                    Some(acc) => acc.min(this), // 不需要掩码，因为access_id放置在高位
                };
                Some(ret)
            })
            .unwrap();
        let sig = Sig((ans & ((1 << SIG_MAXBIT) - 1)) as u32);
        sig.check();
        let cur = self.table[sig.0 as usize - 32].0.unwrap();
        self.remove_node(sig, cur);
        unsafe { Node::free(cur) };
        Some(sig)
    }
    pub fn fork(&self) -> Self {
        let mut rtq = Self::new();
        let mut cur = self.head;
        while let Some(this) = cur {
            unsafe {
                let sig = (*this.as_ptr()).sig();
                rtq.receive(sig);
                cur = (*this.as_ptr()).next;
            }
        }
        rtq
    }
}
