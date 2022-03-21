use core::{mem::MaybeUninit, ptr::NonNull};

use alloc::boxed::Box;

use crate::tools::container::thread_local_linked_list::ThreadLocalLinkedList;

use super::marked_ptr::{AtomicMarkedPtr, MarkedPtr, PtrID};

/// 无锁单向链表
pub struct LockfreeStack<T> {
    /// 使用 INVAILD 表示关闭
    head: AtomicMarkedPtr<LockfreeNode<T>>,
}
unsafe impl<T> Send for LockfreeStack<T> {}
unsafe impl<T> Sync for LockfreeStack<T> {}

struct LockfreeNode<T> {
    next: AtomicMarkedPtr<Self>,
    value: MaybeUninit<T>,
}

impl<T> LockfreeStack<T> {
    pub const fn new() -> Self {
        Self {
            head: AtomicMarkedPtr::null(),
        }
    }
    pub fn take(&self) -> Result<ThreadLocalLinkedList<T>, ()> {
        let mut head = self.head.load();
        loop {
            head.valid()?;
            let null = MarkedPtr::null(head.id());
            match self.head.compare_exchange(head, null) {
                Ok(_) => {
                    let head = head.cast();
                    return Ok(ThreadLocalLinkedList::ptr_new(head));
                }
                Err(cur_head) => {
                    head = cur_head;
                    core::hint::spin_loop();
                }
            }
        }
    }
    pub fn replace(
        &self,
        list: ThreadLocalLinkedList<T>,
    ) -> Result<ThreadLocalLinkedList<T>, ThreadLocalLinkedList<T>> {
        let mut head = self.head.load();
        loop {
            match head.valid() {
                Ok(_) => (),
                Err(_) => return Err(list),
            }
            let new_head = MarkedPtr::new(head.id(), list.head().get_ptr());
            match self.head.compare_exchange(head, new_head) {
                Ok(_) => {
                    let head = head.cast();
                    return Ok(ThreadLocalLinkedList::ptr_new(head));
                }
                Err(cur_head) => {
                    head = cur_head;
                    core::hint::spin_loop();
                }
            }
        }
    }
    pub fn appand(&self, list: &mut ThreadLocalLinkedList<T>) -> Result<(), ()> {
        let list_tail = list.tail_pointer().ok_or(())?;
        let mut head = self.head.load();
        loop {
            match head.valid() {
                Ok(_) => (),
                Err(_) => {
                    unsafe { (*list_tail).into_null() };
                    return Err(());
                }
            }
            unsafe { (*list_tail) = head };
            let new_head = MarkedPtr::new(head.id(), list.head().get_ptr());
            match self.head.compare_exchange(head, new_head) {
                Ok(_) => {
                    unsafe { list.leak_reset() };
                    return Ok(());
                }
                Err(cur_head) => {
                    head = cur_head;
                    core::hint::spin_loop();
                }
            }
        }
    }

    pub fn close(&self) -> Result<ThreadLocalLinkedList<T>, ()> {
        let mut head = self.head.load();
        loop {
            head.valid()?;
            let invalid = MarkedPtr::new_invalid(head.id());
            match self.head.compare_exchange(head, invalid) {
                Ok(_) => {
                    let head = head.cast();
                    return Ok(ThreadLocalLinkedList::ptr_new(head));
                }
                Err(cur_head) => {
                    head = cur_head;
                    core::hint::spin_loop();
                }
            }
        }
    }
    pub fn push(&self, value: T) -> Result<(), ()> {
        stack_trace!();
        let node = LockfreeNode::<T> {
            next: AtomicMarkedPtr::null(),
            value: MaybeUninit::new(value),
        };
        let mut new_node: NonNull<LockfreeNode<T>> = Box::leak(Box::new(node)).into();
        let mut head = self.head.load();
        loop {
            head.valid()?;
            unsafe {
                new_node.as_mut().next =
                    AtomicMarkedPtr::new(MarkedPtr::new(PtrID::zero(), head.get_ptr()));
            }
            let new_head = MarkedPtr::new(head.id(), Some(new_node));
            match self.head.compare_exchange(head, new_head) {
                Ok(_) => return Ok(()),
                Err(cur_head) => {
                    head = cur_head;
                    core::hint::spin_loop();
                }
            }
        }
    }
    pub fn pop(&self) -> Result<Option<T>, ()> {
        stack_trace!();
        let mut head = self.head.load();
        loop {
            head.valid()?;
            let node = match head.get_ptr() {
                None => return Ok(None),
                Some(value) => unsafe { &*value.as_ptr() },
            };
            let new_head = MarkedPtr::new(head.id(), node.next.load().get_ptr());
            match self.head.compare_exchange(head, new_head) {
                Ok(_) => unsafe {
                    let value = node.value.assume_init_read();
                    Box::from_raw(head.get_ptr().unwrap().as_ptr());
                    return Ok(Some(value));
                },
                Err(cur_head) => {
                    head = cur_head;
                    core::hint::spin_loop();
                }
            }
        }
    }
}

pub mod test {
    use super::LockfreeStack;

    pub fn base_test() {
        let list = LockfreeStack::new();
        for i in [1, 2, 3, 4, 5, 6, 7] {
            list.push(i).unwrap();
        }
        for &i in [1, 2, 3, 4, 5, 6, 7].iter().rev() {
            assert_eq!(list.pop().unwrap(), Some(i));
        }
        assert_eq!(list.pop().unwrap(), None);
    }
}
