pub mod fast_clone_linked_list;
pub mod intrusive_linked_list;
pub mod lockfree;
pub mod never_clone_linked_list;
pub mod pop_smallest_set;
pub mod range_map;
pub mod sync_unsafe_cell;
pub mod thread_local_linked_list;

pub trait Stack<T> {
    fn push(&mut self, data: T);
    fn pop(&mut self) -> Option<T>;
}

pub fn test() {
    intrusive_linked_list::test::test();
    lockfree::stack::test::base_test();
}

pub fn multi_thread_performance_test(hart: usize) {
    lockfree::queue::test::multi_thread_performance_test(hart, 4);
}

pub fn multi_thread_stress_test(hart: usize) {
    lockfree::queue::test::multi_thread_stress_test(hart, 4);
}
