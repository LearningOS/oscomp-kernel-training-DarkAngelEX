use core::{mem::MaybeUninit, ptr::NonNull};

use alloc::boxed::Box;

use super::marked_ptr::{MarkedPtr, PtrID};

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
    pub fn ptr_new(ptr: MarkedPtr<ThreadLocalNode<T>>) -> Self {
        Self { head: ptr }
    }
    pub fn push(&mut self, value: T) {
        let node = ThreadLocalNode::<T> {
            next: MarkedPtr::new(PtrID::zero(), self.head.get_ptr()),
            value: MaybeUninit::new(value),
        };
        let new_node: NonNull<ThreadLocalNode<T>> = Box::leak(Box::new(node)).into();
        self.head = MarkedPtr::new(PtrID::zero(), Some(new_node));
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
    pub fn tail_pointer(&self) -> *mut MarkedPtr<ThreadLocalNode<T>> {
        let mut cur: *mut MarkedPtr<ThreadLocalNode<T>> = &self.head as *const _ as *mut _;
        unsafe {
            while let Some(value) = (*cur).get_ptr() {
                cur = value.as_ptr() as *mut _;
            }
        }
        cur
    }
}
