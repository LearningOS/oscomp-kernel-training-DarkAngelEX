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
/// 进来就释放的栈
#[derive(Clone, Copy)]
pub struct LeakStack;
impl<T> Stack<T> for LeakStack {
    fn push(&mut self, _data: T) {}
    fn pop(&mut self) -> Option<T> {
        None
    }
}
impl const Default for LeakStack {
    fn default() -> Self {
        Self
    }
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
