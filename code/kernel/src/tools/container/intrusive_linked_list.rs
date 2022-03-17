use core::ptr::NonNull;

use crate::xdebug::trace;

// 侵入式链表
pub struct IntrusiveLinkedList {
    head: Option<NonNull<usize>>,
    tail: *mut usize, // used in O(1) append. if is invalid when head is none.
    size: usize,
}

unsafe impl Send for IntrusiveLinkedList {}

impl Drop for IntrusiveLinkedList {
    fn drop(&mut self) {
        // 防止内存泄露
        // 不能回收 因为这个链表就是用来写内存分配器的
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
    pub fn from_range(begin: usize, end: usize, align_log2: usize) -> Self {
        let size = 1 << align_log2;
        let mut list = Self::new();
        let mut cur = end;
        while cur != begin {
            let next = cur - size;
            unsafe { list.push(NonNull::new_unchecked(next as *mut _)) };
            cur = next;
        }
        list
    }
    pub fn empty_forward(self) -> Option<Self> {
        if self.is_empty() {
            return None;
        }
        Some(self)
    }
    pub fn len(&self) -> usize {
        self.size
    }
    // Err: (target_size, missing_size)
    pub fn size_check(&self) -> Result<usize, (usize, isize)> {
        let mut x = self.head;
        let target = self.size;
        let mut cnt = target;
        while cnt > 0 {
            x = unsafe { *x.ok_or((target, cnt as isize))?.cast().as_ptr() };
            cnt -= 1;
        }
        if x.is_some() {
            return Err((target, -1));
        }
        Ok(self.size)
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
    pub fn take(&mut self, n: usize) -> Self {
        let mut new_list = Self::new();
        for _i in 0..n {
            if let Some(ptr) = self.pop() {
                unsafe { new_list.push(ptr) };
            } else {
                break;
            }
        }
        new_list
    }
    pub fn for_each_impl(node: Option<NonNull<usize>>, mut f: impl FnMut(NonNull<usize>)) {
        let mut cur = node;
        while let Some(value) = cur {
            f(value);
            cur = unsafe { *value.cast().as_ptr() };
        }
    }
    pub fn for_each(&self, f: impl FnMut(NonNull<usize>)) {
        Self::for_each_impl(self.head, f);
    }
    fn node_iter(&mut self) -> NodeIter {
        NodeIter {
            pointer: &mut self.head,
        }
    }
    pub fn sort_no_buffer(&mut self) {
        if let Some(x) = self.head {
            self.head = unsafe { Some(merge_sort(x, self.len())) };
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
        unsafe fn merge_sort(head: Node, max_len: usize) -> Node {
            if next_v(head).is_none() {
                return head;
            }
            // stack_trace!(); // this will result in dead_lock!
            trace::stack_detection();
            let mut p = head;
            let mut q = Some(head);
            let mut pre = None;
            let mut cnt = 0;
            loop {
                assert!(cnt < max_len);
                cnt += 1;
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
            let l = merge_sort(head, max_len);
            let r = merge_sort(p, max_len);
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

    pub fn sort_with_buffer(&mut self, buffer: &mut [usize]) {
        let size = self.size;
        if size <= 1 {
            return;
        }
        assert!(size <= buffer.len());

        let buffer = &mut buffer[0..size];
        let mut cur = self.head;
        for v in buffer.iter_mut() {
            unsafe {
                *v = core::mem::transmute(cur);
                cur = *cur.unwrap().cast().as_ptr();
            }
        }
        assert!(cur.is_none());
        buffer.sort_unstable();
        assert!(buffer[0] != 0);
        unsafe {
            self.head = core::mem::transmute(buffer[0]);
            for a in buffer.windows(2) {
                let p = a[0] as *mut usize;
                *p = a[1];
            }
            let p = buffer[size - 1] as *mut usize;
            *p = 0;
        }
    }

    /// returned list will be sorted by large-first.
    pub fn collection(&mut self, align_log2: usize) -> IntrusiveLinkedList {
        let mask = 1 << align_log2;
        let mut list = IntrusiveLinkedList::new();
        self.size_check().unwrap();
        self.sort_no_buffer();
        let mut node_iter = self.node_iter();
        while let Some((a, b)) = node_iter.current_and_next() {
            debug_check!(a < b);
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
            None
        }
    }
    pub fn next(&mut self) -> Result<(), ()> {
        unsafe {
            if let Some(a) = *self.pointer {
                self.pointer = a.as_ptr().cast();
                return Ok(());
            }
        }
        Err(())
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
            list.sort_no_buffer();

            for x in test_set.iter_mut() {
                let v = list.pop().unwrap();
                *x = anti(v);
                print!("{} ", x);
            }
            println!();
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
            new_list.sort_no_buffer();
            xprint(&list);
            print!(" ");
            xprint(&new_list);
            println!();
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
            sort_test(&mut [1]);
            sort_test(&mut [1, 2]);
            sort_test(&mut [2, 1]);
            sort_test(&mut [2, 3, 1]);
            sort_test(&mut [1, 2, 3, 4, 5]);
            sort_test(&mut [5, 4, 3, 2, 1]);
            sort_test(&mut [1, 8, 45, 13, 56, 15, 489, 12, 68, 74, 23, 49]);
            sort_test(&mut [5, 12, 68, 74, 23, 49, 1, 8, 45, 13, 56, 15, 489]);
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
