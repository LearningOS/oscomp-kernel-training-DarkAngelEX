use core::{mem::MaybeUninit, ptr::NonNull};

use alloc::boxed::Box;

use super::lockfree::marked_ptr::{MarkedPtr, PtrID};

pub struct ThreadLocalLinkedList<T> {
    head: MarkedPtr<ThreadLocalNode<T>>,
}

impl<T> Drop for ThreadLocalLinkedList<T> {
    fn drop(&mut self) {
        assert_eq!(self.head.get_ptr(), None);
    }
}

pub struct ThreadLocalNode<T> {
    next: MarkedPtr<Self>,
    value: MaybeUninit<T>,
}

impl<T> ThreadLocalLinkedList<T> {
    pub fn empty() -> Self {
        Self {
            head: MarkedPtr::null(PtrID::zero()),
        }
    }
    pub(super) fn ptr_new(ptr: MarkedPtr<ThreadLocalNode<T>>) -> Self {
        Self { head: ptr }
    }
    pub fn push(&mut self, value: T) {
        let id = self.head.id();
        let node = ThreadLocalNode::<T> {
            next: MarkedPtr::new(id, self.head.get_ptr()),
            value: MaybeUninit::new(value),
        };
        let new_node: NonNull<ThreadLocalNode<T>> = Box::leak(Box::new(node)).into();
        self.head = MarkedPtr::new(id.next(), Some(new_node));
    }
    pub fn pop(&mut self) -> Option<T> {
        let node = match self.head.get_ptr() {
            None => return None,
            Some(value) => unsafe { &mut *value.as_ptr() },
        };
        unsafe {
            let ThreadLocalNode { next, value } = *Box::from_raw(node);
            self.head = next;
            Some(value.assume_init())
        }
    }
    pub fn len(&self) -> usize {
        let mut x = self.head;
        let mut n = 0;
        while let Some(mut value) = x.get_ptr() {
            x = unsafe { value.as_mut().next };
            n += 1;
        }
        n
    }
    pub(super) unsafe fn leak_reset(&mut self) {
        self.head = self.head.into_null()
    }
    pub(super) fn head<A>(&self) -> MarkedPtr<A> {
        self.head.cast()
    }
    /// 当链表为空时返回None.
    ///
    /// 时间复杂度为O(N) 因为数据结构没有保存tail指针.
    pub(super) fn head_tail<A, B>(&self) -> Option<(MarkedPtr<A>, *mut B)> {
        let head = self.head.get_ptr()?;
        let mut tail = head;
        unsafe {
            while let Some(value) = tail.as_mut().next.get_ptr() {
                tail = value;
            }
        }
        Some((self.head.cast(), tail.cast().as_ptr()))
    }
    /// 时间复杂度为O(N) 因为数据结构没有保存tail指针.
    pub(super) fn tail_pointer<A>(&self) -> Option<*mut MarkedPtr<A>> {
        self.head.get_ptr()?;
        let mut cur: *mut MarkedPtr<A> = &self.head as *const _ as *mut _;
        unsafe {
            while let Some(value) = (*cur).get_ptr() {
                cur = value.as_ptr() as *mut _;
            }
        }
        Some(cur)
    }
}
