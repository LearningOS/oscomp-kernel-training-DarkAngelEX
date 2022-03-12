use crate::tools::container::lock_free_queue::LockFreeQueue;

pub mod never_clone_linked_list;
pub mod fast_clone_linked_list;
pub mod sync_unsafe_cell;
pub mod pop_smallest_set;
pub mod lock_free_queue;
pub mod lock_free_stack;
pub mod marked_ptr;
pub mod intrusive_linked_list;

pub trait Stack<T> {
    fn push(&mut self, data: T);
    fn pop(&mut self) -> Option<T>;
}

pub fn test() {
    intrusive_linked_list::test::test();
}

pub fn multi_thread_test(hart: usize) {
    lock_free_queue::test::multi_thread_test(hart, 4);
}
