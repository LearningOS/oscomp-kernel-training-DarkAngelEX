use core::ptr::NonNull;

// 侵入式链表
pub struct IntrusiveLinkedList {
    head: Option<NonNull<usize>>,
    tail: *mut usize, // used in O(1) append. if is invalid when head is none.
    size: usize,
}
impl Drop for IntrusiveLinkedList {
    fn drop(&mut self) {
        // 强制析构前释放全部数据
        debug_check!(self.head.is_none());
    }
}

struct NodeIter {
    pointer: *mut Option<NonNull<usize>>,
}

impl IntrusiveLinkedList {
    pub const fn new() -> Self {
        Self {
            head: None,
            tail: core::ptr::null_mut(),
            size: 0,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.head.is_none()
    }
    pub fn len(&self) -> usize {
        self.size
    }
    pub fn size_reset(&mut self, new_size: usize) {
        self.size = new_size;
    }
    pub unsafe fn push(&mut self, ptr: NonNull<usize>) {
        *ptr.as_ptr() = core::mem::transmute(self.head);
        if self.is_empty() {
            self.tail = ptr.as_ptr();
        }
        self.head = Some(ptr);
        self.size += 1;
    }
    pub fn pop(&mut self) -> Option<NonNull<usize>> {
        match self.head {
            None => None,
            Some(ptr) => {
                let next = unsafe { *ptr.as_ptr() };
                self.head = NonNull::new(next as *mut _);
                self.size -= 1;
                Some(ptr)
            }
        }
    }
    pub fn append(&mut self, src: &mut Self) {
        if src.is_empty() {
            return;
        }
        match self.head {
            None => core::mem::swap(self, src),
            Some(head_ptr) => {
                unsafe { *src.tail = head_ptr.as_ptr() as usize };
                self.head = src.head;
                self.size += src.size;
                src.head = None;
                src.size = 0;
            }
        }
    }
    pub fn for_each(&self, mut f: impl FnMut(NonNull<usize>)) {
        let mut cur = self.head;
        while let Some(value) = cur {
            f(value);
            cur = unsafe { *value.cast().as_ptr() };
        }
    }
    fn node_iter(&mut self) -> NodeIter {
        NodeIter {
            pointer: &mut self.head,
        }
    }
    pub fn sort(&mut self) {
        if let Some(x) = self.head {
            self.head = unsafe { Some(merge_sort(x)) };
        }

        type Node = NonNull<usize>;

        unsafe fn next(this: Node) -> *mut Option<Node> {
            this.cast().as_ptr()
        }
        unsafe fn next_v(this: Node) -> Option<Node> {
            *next(this)
        }
        unsafe fn value(this: Node) -> usize {
            this.as_ptr() as usize
        }
        unsafe fn merge_sort(head: Node) -> Node {
            if next_v(head).is_none() {
                return head;
            }
            stack_trace!();
            let mut p = head;
            let mut q = Some(head);
            let mut pre = None;
            loop {
                if let Some(qn) = q {
                    if let Some(qnn) = next_v(qn) {
                        pre = Some(p);
                        p = next_v(p).unwrap();
                        q = next_v(qnn);
                        continue;
                    }
                }
                break;
            }
            if let Some(pre) = pre {
                *next(pre) = None;
            } else {
                // list have only one node.
                return head;
            }
            let l = merge_sort(head);
            let r = merge_sort(p);
            merge(l, r)
        }
        #[inline(never)]
        unsafe fn merge(l: Node, r: Node) -> Node {
            let (a, mut l) = take_smallest(l);
            let (b, mut r) = take_smallest(r);
            let first = if value(a) < value(b) {
                *next(b) = r;
                r = Some(b);
                a
            } else {
                *next(a) = l;
                l = Some(a);
                b
            };
            *next(first) = None;
            let mut cur = first;

            while let (Some(xl), Some(xr)) = (l, r) {
                if value(xl) < value(xr) {
                    *next(cur) = l;
                    cur = xl;
                    l = next_v(xl);
                } else {
                    *next(cur) = r;
                    cur = xr;
                    r = next_v(xr);
                }
            }

            match (l, r) {
                (Some(_), None) => *next(cur) = l,
                (None, Some(_)) => *next(cur) = r,
                _ => (),
            }
            first
        }
        unsafe fn take_smallest(head: Node) -> (Node, Option<Node>) {
            let mut sel = head;
            let mut cur = head;
            let mut pre = None;
            while let Some(nxt) = next_v(cur) {
                let old = cur;
                cur = nxt;
                if value(cur) < value(sel) {
                    sel = cur;
                    pre = Some(old);
                }
            }
            if let Some(pre) = pre {
                *next(pre) = next_v(sel);
                (sel, Some(head))
            } else {
                (head, next_v(head))
            }
        }
    }

    /// returned list will be sorted by large-first.
    pub fn collection(&mut self, align: usize) -> IntrusiveLinkedList {
        let mask = 1 << align;
        let mut list = IntrusiveLinkedList::new();
        self.sort();
        let mut node_iter = self.node_iter();
        while let Some((a, b)) = node_iter.current_and_next() {
            debug_assert!(a < b);
            if (a.as_ptr() as usize ^ b.as_ptr() as usize) == mask {
                node_iter.remove_current_and_next();
                unsafe { list.push(a) };
                continue;
            }
            if node_iter.next().is_err() {
                break;
            }
        }
        self.size_reset(self.len() - list.len() * 2);
        list
    }
}

impl NodeIter {
    pub fn current_and_next(&self) -> Option<(NonNull<usize>, NonNull<usize>)> {
        unsafe {
            if let Some(a) = *self.pointer {
                if let Some(b) = NonNull::new(*a.as_ptr() as *mut _) {
                    return Some((a, b));
                }
            }
            return None;
        }
    }
    pub fn next(&mut self) -> Result<(), ()> {
        unsafe {
            if let Some(a) = *self.pointer {
                self.pointer = a.as_ptr().cast();
                return Ok(());
            }
        }
        return Err(());
    }
    // 释放连续两个节点并返回第一个的值 需要保证第二个节点的空间不会被立刻使用。
    pub fn remove_current_and_next(&mut self) {
        unsafe {
            if let Some(a) = *self.pointer {
                if let Some(b) = NonNull::new(*a.cast().as_ptr()) {
                    *self.pointer = NonNull::new(*b.as_ptr());
                    return;
                }
            }
            panic!();
        }
    }
}

pub mod test {
    use core::ptr::NonNull;

    use alloc::vec;

    use crate::{
        memory::allocator::frame, tools::container::intrusive_linked_list::IntrusiveLinkedList,
    };

    fn sort_test(test_set: &mut [usize]) {
        let page = frame::global::alloc().unwrap();
        let array = page.data().as_usize_array_mut();
        let begin = &array[0] as *const _ as usize;

        let tran = |a: usize| {
            assert!(a < 512);
            NonNull::new((begin + a * 8) as *mut usize).unwrap()
        };
        let anti = |a: NonNull<usize>| (a.as_ptr() as usize - begin) / 8;

        unsafe {
            let mut list = IntrusiveLinkedList::new();
            for &x in test_set.iter() {
                assert!(x < 512);
                list.push(tran(x));
            }
            list.sort();

            for x in test_set.iter_mut() {
                let v = list.pop().unwrap();
                *x = anti(v);
                print!("{} ", x);
            }
            print!("\n");
            assert_eq!(list.pop(), None);
            for x in test_set.windows(2) {
                assert!(x[0] < x[1]);
            }
        }
    }
    fn collection_test(test_set: &[usize], leave: &[usize], new: &[usize]) {
        let page = frame::global::alloc().unwrap();
        let array = page.data().as_usize_array_mut();
        let begin = &array[0] as *const _ as usize;

        let tran = move |a: usize| {
            assert!(a < 512);
            NonNull::new((begin + a * 8) as *mut usize).unwrap()
        };
        let anti = move |a: NonNull<usize>| (a.as_ptr() as usize - begin) / 8;
        let print_impl = |a| {
            print!("{},", anti(a));
        };
        let xprint = |list: &IntrusiveLinkedList| {
            print!("[");
            list.for_each(print_impl);
            print!("]");
        };
        unsafe {
            let mut list = IntrusiveLinkedList::new();
            for &x in test_set.iter() {
                list.push(tran(x));
            }
            xprint(&list);
            print!(" -> ");
            let mut new_list = list.collection(3);
            new_list.sort();
            xprint(&list);
            print!(" ");
            xprint(&new_list);
            print!("\n");
            for &x in leave.iter() {
                let v = list.pop().unwrap();
                assert_eq!(x, anti(v));
            }
            assert_eq!(list.pop(), None);
            for &x in new.iter() {
                let v = new_list.pop().unwrap();
                assert_eq!(x, anti(v));
            }
            assert_eq!(new_list.pop(), None);
        }
    }
    pub fn test() {
        stack_trace!();
        if false {
            println!("IntrusiveLinkedList sort_test begin");
            sort_test(&mut vec![1]);
            sort_test(&mut vec![1, 2]);
            sort_test(&mut vec![2, 1]);
            sort_test(&mut vec![2, 3, 1]);
            sort_test(&mut vec![1, 2, 3, 4, 5]);
            sort_test(&mut vec![5, 4, 3, 2, 1]);
            sort_test(&mut vec![1, 8, 45, 13, 56, 15, 489, 12, 68, 74, 23, 49]);
            sort_test(&mut vec![5, 12, 68, 74, 23, 49, 1, 8, 45, 13, 56, 15, 489]);
            println!("IntrusiveLinkedList sort_test pass");
            println!("IntrusiveLinkedList collection_test begin");
            collection_test(&[2, 4], &[2, 4], &[]);
            collection_test(&[2, 3], &[], &[2]);
            collection_test(&[0, 1, 2, 4, 5, 6], &[2, 6], &[0, 4]);
            collection_test(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10], &[10], &[0, 2, 4, 6, 8]);
            collection_test(&[0, 1, 2, 4, 5, 6, 8, 9, 10], &[2, 6, 10], &[0, 4, 8]);

            println!("IntrusiveLinkedList collection_test pass");
            panic!("test complete");
        } else {
            println!("IntrusiveLinkedList test skip");
        }
    }
}
