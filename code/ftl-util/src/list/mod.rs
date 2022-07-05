pub mod access;
#[macro_use]
mod intrusive;

use core::{marker::PhantomPinned, ptr::NonNull};

pub use intrusive::InListNode;

/// 侵入式链表节点
///
/// ListNode 必须在使用前使用 init 或 lazy_init 初始化
pub struct ListNode<T> {
    prev: *mut ListNode<T>,
    next: *mut ListNode<T>,
    data: T,
    _marker: PhantomPinned,
}

unsafe impl<T> Send for ListNode<T> {}

impl<T> ListNode<T> {
    #[inline(always)]
    pub const fn new(data: T) -> Self {
        Self {
            prev: core::ptr::null_mut(),
            next: core::ptr::null_mut(),
            data,
            _marker: PhantomPinned,
        }
    }
    #[inline(always)]
    pub fn init(&mut self) {
        self.prev = self;
        self.next = self;
    }
    #[inline(always)]
    pub fn lazy_init(&mut self) {
        if self.prev.is_null() {
            debug_assert!(self.next.is_null());
            self.init();
        }
        debug_assert!(!self.next.is_null());
    }
    pub fn list_check(&self) {
        if cfg!(debug_assertions) {
            unsafe {
                debug_assert!(!self.prev.is_null());
                debug_assert!(!self.next.is_null());
                let mut cur = self as *const _ as *mut Self;
                let mut nxt = (*cur).next;
                assert!((*nxt).prev == cur);
                cur = nxt;
                nxt = (*cur).next;
                while cur.as_const() != self {
                    assert!((*nxt).prev == cur);
                    cur = nxt;
                    nxt = (*cur).next;
                }
            }
        }
    }
    #[inline(always)]
    pub fn data(&self) -> &T {
        &self.data
    }
    #[inline(always)]
    pub fn data_mut(&mut self) -> &mut T {
        &mut self.data
    }
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        if self.prev.as_const() == self {
            debug_assert!(self.next.as_const() == self);
            true
        } else {
            debug_assert!(self.next.as_const() != self);
            false
        }
    }
    #[inline(always)]
    pub fn push_prev(&mut self, new: &mut Self) {
        debug_assert!(self as *mut _ != new as *mut _);
        debug_assert!(new.is_empty());
        new.prev = self.prev;
        new.next = self;
        debug_assert!(unsafe { (*self.prev).next == self });
        unsafe { (*self.prev).next = new };
        self.prev = new;
    }
    #[inline(always)]
    pub fn push_next(&mut self, new: &mut Self) {
        debug_assert!(self as *mut _ != new as *mut _);
        debug_assert!(new.is_empty());
        new.prev = self;
        new.next = self.next;
        debug_assert!(unsafe { (*self.next).prev == self });
        unsafe { (*self.next).prev = new };
        self.next = new;
    }
    #[inline(always)]
    pub fn try_prev(&self) -> Option<NonNull<Self>> {
        if self.is_empty() {
            return None;
        }
        NonNull::new(self.prev)
    }
    #[inline(always)]
    pub fn try_next(&self) -> Option<NonNull<Self>> {
        if self.is_empty() {
            return None;
        }
        NonNull::new(self.next)
    }
    #[inline(always)]
    pub fn pop_self(&mut self) {
        let prev = self.prev;
        let next = self.next;
        unsafe {
            (*prev).next = next;
            (*next).prev = prev;
        }
        self.init();
    }
    #[inline(always)]
    pub fn pop_prev(&mut self) -> Option<NonNull<Self>> {
        if self.is_empty() {
            return None;
        }
        let r = self.prev;
        unsafe {
            debug_assert!((*r).next == self);
            let r_prev = (*r).prev;
            debug_assert!((*r_prev).next == r);
            self.prev = r_prev;
            (*r_prev).next = self;
            (*r).init();
        }
        NonNull::new(r)
    }
    #[inline(always)]
    pub fn pop_next(&mut self) -> Option<NonNull<Self>> {
        if self.is_empty() {
            return None;
        }
        let r = self.next;
        unsafe {
            debug_assert!((*r).prev == self);
            let r_next = (*r).next;
            debug_assert!((*r_next).prev == r);
            self.next = r_next;
            (*r_next).prev = self;
            (*r).init();
        }
        NonNull::new(r)
    }
    /// 按next序扫描一圈, 当 need_pop 返回true时删除这个节点, 删除后再调用 release, 至多删除max个节点
    ///
    /// 分离need_pop和release是为了保证release调用时不存在任何引用, 因为release调用时node可能已经被销毁了
    #[inline]
    pub fn pop_many_when(
        &mut self,
        max: usize,
        mut need_pop: impl FnMut(&T) -> bool,
        mut release: impl FnMut(&mut T),
    ) -> usize {
        let mut n = 0;
        unsafe {
            let head = self as *mut Self;
            let mut cur = (*head).next;
            while cur != head {
                let next = (*cur).next;
                if need_pop(&(*cur).data) {
                    (*cur).pop_self();
                    release(&mut (*cur).data);
                    n += 1;
                    if n == max {
                        return n;
                    }
                }
                cur = next;
            }
        }
        n
    }
}
