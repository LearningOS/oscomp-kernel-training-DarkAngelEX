use core::{marker::PhantomData, ptr::NonNull};

use super::{access::ListAccess, ListNode};

/// 生成一个通过成员反向获取父类的类型
#[macro_export]
macro_rules! inlist_access {
    ($name: ident, $T: ty, $field:ident) => {
        struct $name {}
        impl $crate::list::access::ListAccess<$T, $crate::list::intrusive::InListNode<$T, Self>>
            for $name
        {
            #[inline(always)]
            fn offset() -> usize {
                $crate::offset_of!($T, $field)
            }
        }
    };
}

/// 侵入式链表头节点
pub struct InListNode<T, A: ListAccess<T, Self>> {
    node: ListNode<PhantomData<(T, A)>>,
}

impl<T, A: ListAccess<T, Self>> InListNode<T, A> {
    pub const fn new() -> Self {
        Self {
            node: ListNode::new(PhantomData),
        }
    }
    /// unsafe:
    ///
    /// &mut A(1, 2) -> (&mut A.1, &A.2)
    ///
    /// &A.2 -> &A(1, 2) -> &A.1
    ///
    /// now we hold &mut A.1 and &A.1 at the same time.
    pub unsafe fn access(&self) -> &T {
        A::get(self)
    }
    /// unsafe:
    ///
    /// &mut A(1, 2) -> (&mut A.1, &mut A.2)
    ///
    /// &mut A.2 -> &mut A(1, 2) -> &mut A.1
    ///
    /// now we hold two &mut A.1 at the same time.
    pub unsafe fn access_mut(&mut self) -> &mut T {
        A::get_mut(self)
    }
    pub fn init(&mut self) {
        self.node.init()
    }
    pub fn lazy_init(&mut self) {
        self.node.lazy_init()
    }
    pub fn list_check(&self) {
        self.node.list_check();
    }
    pub fn is_empty(&self) -> bool {
        self.node.is_empty()
    }
    pub fn push_prev(&mut self, new: &mut Self) {
        self.node.push_prev(&mut new.node)
    }
    pub fn push_next(&mut self, new: &mut Self) {
        self.node.push_next(&mut new.node)
    }
    pub fn try_prev(&self) -> Option<NonNull<Self>> {
        unsafe { core::mem::transmute(self.node.try_prev()) }
    }
    pub fn try_next(&self) -> Option<NonNull<Self>> {
        unsafe { core::mem::transmute(self.node.try_next()) }
    }
    pub fn pop_self(&mut self) {
        self.node.pop_self()
    }
    pub fn pop_prev(&mut self) -> Option<NonNull<Self>> {
        unsafe { core::mem::transmute(self.node.pop_prev()) }
    }
    pub fn pop_next(&mut self) -> Option<NonNull<Self>> {
        unsafe { core::mem::transmute(self.node.pop_next()) }
    }
}
