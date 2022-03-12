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

pub struct NodeIter {
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
    pub fn sort(&mut self) {
        todo!()
    }
    pub fn node_iter(&mut self) -> NodeIter {
        NodeIter {
            pointer: &mut self.head,
        }
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
            }
        }
        return Err(());
    }
    // 释放连续两个节点并返回第一个的值 需要保证第二个节点的空间不会被立刻使用。
    pub fn remove_current_and_next(&mut self) -> NonNull<usize> {
        unsafe {
            if let Some(a) = *self.pointer {
                if let Some(b) = NonNull::new(*a.as_ptr() as *mut _) {
                    self.pointer = b.as_ptr();
                    return a;
                }
            }
            panic!();
        }
    }
}
