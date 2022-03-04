pub mod never_clone_linked_list;
pub mod fast_clone_linked_list;

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
}
