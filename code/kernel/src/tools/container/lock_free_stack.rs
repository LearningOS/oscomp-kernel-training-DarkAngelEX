use super::marked_ptr::AtomicMarkedPtr;

// 无锁单向链表
pub struct LockFreeStack<T> {
    head: AtomicMarkedPtr<T>,
}

impl<T> LockFreeStack<T> {
    pub const fn new() -> Self {
        Self {
            head: AtomicMarkedPtr::null(),
        }
    }
}
