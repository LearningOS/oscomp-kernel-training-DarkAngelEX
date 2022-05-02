use core::ptr::NonNull;

/// 侵入式链表节点
///
/// ListNode 必须在使用前使用 init 或 lazy_init 初始化
///
/// 由于new函数将导致ListNode被移动, 因此不能在new中初始化指针
pub struct ListNode<T> {
    prev: *mut ListNode<T>,
    next: *mut ListNode<T>,
    data: T,
}

unsafe impl<T> Send for ListNode<T> {}

impl<T> ListNode<T> {
    pub const fn new(data: T) -> Self {
        Self {
            prev: core::ptr::null_mut(),
            next: core::ptr::null_mut(),
            data,
        }
    }
    pub fn init(&mut self) {
        self.prev = self;
        self.next = self;
    }
    pub fn lazy_init(&mut self) {
        if self.prev.is_null() {
            debug_assert!(self.next.is_null());
            self.init();
        }
        debug_assert!(!self.next.is_null());
    }
    pub fn data(&self) -> &T {
        &self.data
    }
    pub fn data_mut(&mut self) -> &mut T {
        &mut self.data
    }
    pub fn is_empty(&self) -> bool {
        if self.prev != self.next {
            return false;
        }
        debug_assert!(!self.prev.is_null());
        if cfg!(debug_assert) && self.prev.as_const() != self {
            // 唯一的可能是链表长度为2
            let other = unsafe { &*self.next };
            let this = self as *const _ as *mut _;
            assert!(other.prev == this);
            assert!(other.next == this);
        }
        true
    }
    pub fn insert_prev(&mut self, new: &mut Self) {
        debug_assert!(new.is_empty());
        new.prev = self.prev;
        new.next = self;
        unsafe { (*self.prev).next = new };
        self.prev = new;
    }
    pub fn insert_next(&mut self, new: &mut Self) {
        debug_assert!(new.is_empty());
        new.prev = self;
        new.next = self.next;
        unsafe { (*self.next).prev = new };
        self.next = new;
    }
    pub fn try_prev(&self) -> Option<NonNull<Self>> {
        if self.is_empty() {
            return None;
        }
        NonNull::new(self.prev)
    }
    pub fn try_next(&self) -> Option<NonNull<Self>> {
        if self.is_empty() {
            return None;
        }
        NonNull::new(self.next)
    }
    pub fn remove_self(&mut self) {
        let prev = self.prev;
        let next = self.next;
        unsafe {
            (*prev).next = next;
            (*next).prev = prev;
        }
        self.init();
    }
    pub fn try_remove_prev(&mut self) -> Option<NonNull<Self>> {
        if self.is_empty() {
            return None;
        }
        let r = self.prev;
        unsafe {
            let r_prev = (*r).prev;
            self.prev = r_prev;
            (*r_prev).next = self;
            (*r).init();
        }
        NonNull::new(r)
    }
    pub fn try_remove_next(&mut self) -> Option<NonNull<Self>> {
        if self.is_empty() {
            return None;
        }
        let r = self.next;
        unsafe {
            let r_next = (*r).next;
            self.next = r_next;
            (*r_next).prev = self;
            (*r).init();
        }
        NonNull::new(r)
    }
}
