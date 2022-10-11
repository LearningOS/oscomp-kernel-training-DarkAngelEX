use core::{
    marker::PhantomPinned,
    ptr::NonNull,
    sync::atomic::{self, Ordering},
};

pub use intrusive::InListNode;

pub mod access;
#[macro_use]
pub mod intrusive;

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
unsafe impl<T> Sync for ListNode<T> {}

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
    pub fn inited(&self) -> bool {
        !self.prev.is_null()
    }
    #[inline(always)]
    pub fn lazy_init(&mut self) {
        if !self.inited() {
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
                while cur.cast_const() != self {
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
        debug_assert!(self.inited());
        if self.prev.cast_const() == self {
            debug_assert!(self.next.cast_const() == self);
            true
        } else {
            debug_assert!(self.next.cast_const() != self);
            false
        }
    }
    /// 无锁竞争状态下使用的判断方法, 不检测next
    ///
    /// 一旦pop后禁止再次被push
    pub fn is_empty_race(&self) -> bool {
        unsafe { core::ptr::read_volatile(&self.prev) }.cast_const() == self
    }
    /// # Safety
    ///
    /// 自行保证指针的安全
    pub unsafe fn set_prev(&mut self, prev: *mut Self) {
        self.prev = prev;
    }
    /// # Safety
    ///
    /// 自行保证指针的安全
    pub unsafe fn set_next(&mut self, next: *mut Self) {
        self.next = next;
    }
    pub fn get_prev(&self) -> *mut Self {
        self.prev
    }
    pub fn get_next(&self) -> *mut Self {
        self.next
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
        debug_assert!(unsafe { (*self.prev).next == self });
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
    pub fn push_next_rcu(&mut self, new: &mut Self) {
        debug_assert!(self as *mut _ != new as *mut _);
        debug_assert!(new.is_empty());
        new.prev = self;
        new.next = self.next;
        atomic::fence(Ordering::Release);
        debug_assert!(unsafe { (*self.next).prev == self });
        debug_assert!(unsafe { (*self.prev).next == self });
        self.next = new;
        unsafe { (*self.next).prev = new };
    }
    /// RCU 节点一旦释放禁止继续使用
    #[inline(always)]
    pub fn pop_self_rcu(&mut self) {
        unsafe {
            self.pop_self_fast();
            self.prev = core::ptr::null_mut();
        }
    }
    /// # Safety
    ///
    /// 此函数不会重置自身, 释放后禁止再被使用
    #[inline(always)]
    pub unsafe fn pop_self_fast(&self) {
        let prev = self.prev;
        let next = self.next;
        (*prev).next = next;
        (*next).prev = prev;
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
    pub fn pop_all(&mut self, mut release: impl FnMut(&mut T)) {
        unsafe {
            let head = self as *mut Self;
            let mut cur = (*head).next;
            while cur != head {
                let next = (*cur).next;
                (*cur).pop_self();
                release((*cur).data_mut());
                cur = next;
            }
        }
    }
    /// release_0在pop之前执行, release_1在pop之后执行
    pub fn pop_when_race(
        &mut self,
        mut need_pop: impl FnMut(&T) -> bool,
        mut release_0: impl FnMut(&mut T),
        mut release_1: impl FnMut(&mut T),
    ) {
        unsafe {
            let head = self as *mut Self;
            let mut cur = (*head).next;
            while cur != head {
                let next = (*cur).next;
                if need_pop(&(*cur).data) {
                    release_0((*cur).data_mut());
                    atomic::fence(Ordering::Release);
                    (*cur).pop_self();
                    atomic::fence(Ordering::Release);
                    release_1((*cur).data_mut());
                }
                cur = next;
            }
        }
    }
    pub fn pop_when(
        &mut self,
        mut need_pop: impl FnMut(&T) -> bool,
        mut release: impl FnMut(&mut T),
    ) {
        unsafe {
            let head = self as *mut Self;
            let mut cur = (*head).next;
            while cur != head {
                let next = (*cur).next;
                if need_pop(&(*cur).data) {
                    (*cur).pop_self();
                    release((*cur).data_mut());
                }
                cur = next;
            }
        }
    }
    /// 详细介绍见 pop_many_ex
    #[inline]
    pub fn pop_many_when(
        &mut self,
        max: usize,
        need_pop: impl FnMut(&T) -> bool,
        mut release: impl FnMut(&mut T),
    ) -> usize {
        self.pop_many_ex(max, need_pop, move |v| release(v.data_mut()))
    }
    /// 按next序扫描一圈, 当 need_pop 返回true时删除这个节点, 删除后再调用 release, 至多删除max个节点
    ///
    /// 分离need_pop和release是为了保证release调用时不存在任何引用, 因为release调用时node可能已经被销毁了
    #[inline]
    pub fn pop_many_ex(
        &mut self,
        max: usize,
        mut need_pop: impl FnMut(&T) -> bool,
        mut release: impl FnMut(&mut ListNode<T>),
    ) -> usize {
        let mut n = 0;
        unsafe {
            let head = self as *mut Self;
            let mut cur = (*head).next;
            while cur != head {
                let next = (*cur).next;
                if need_pop(&(*cur).data) {
                    (*cur).pop_self();
                    release(&mut *cur);
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
