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
    let mut a = never_clone_linked_list::NeverCloneLinkedList::new();
    a.push(1);
    a.push(2);
    a.push(3);
    a.push(4);
    a.push(5);
    a.retain(|a|*a != 5);
    while let Some(x) = a.pop() {
        print!("{} ", x);
    }
    println!("container test end");
    let mut queue = LockFreeQueue::new();
    queue.init();
    queue.push(1).unwrap();
    queue.push(2).unwrap();
    queue.push(3).unwrap();
    queue.push(4).unwrap();
    queue.push(5).unwrap();
    assert_eq!(queue.pop().unwrap().unwrap(), 1);
    assert_eq!(queue.pop().unwrap().unwrap(), 2);
    assert_eq!(queue.pop().unwrap().unwrap(), 3);
    assert_eq!(queue.pop().unwrap().unwrap(), 4);
    assert_eq!(queue.pop().unwrap().unwrap(), 5);
    assert_eq!(queue.pop().unwrap(), None);
}

pub fn multi_thread_test(hart: usize) {
    lock_free_queue::test::multi_thread_test(hart, 4);
}
