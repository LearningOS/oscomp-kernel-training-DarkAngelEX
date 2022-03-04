use core::marker::PhantomData;

use crate::tools::{
    container::{
        fast_clone_linked_list::FastCloneLinkedList, never_clone_linked_list::NeverCloneLinkedList,
        Stack,
    },
    ForwardWrapper, Wrapper,
};

pub trait FromUsize {
    fn from_usize(num: usize) -> Self;
    fn into_usize(&self) -> usize;
}

impl FromUsize for usize {
    fn from_usize(num: usize) -> Self {
        num
    }
    fn into_usize(&self) -> usize {
        *self
    }
}

#[macro_export]
macro_rules! from_usize_impl {
    ($ty_name: ident) => {
        impl crate::tools::allocator::from_usize_allocator::FromUsize for $ty_name {
            fn from_usize(num: usize) -> Self {
                Self(num)
            }
            fn into_usize(&self) -> usize {
                self.0
            }
        }
    };
}

#[derive(Debug, Clone)]
pub struct FromUsizeIter<T: FromUsize> {
    next: usize,
    _marker: PhantomData<T>,
}

impl<T: FromUsize> FromUsizeIter<T> {
    pub const fn new(start: usize) -> Self {
        Self {
            next: start,
            _marker: PhantomData,
        }
    }
    pub fn next(&mut self) -> T {
        let num = self.next;
        let ret = T::from_usize(num);
        self.next += 1;
        ret
    }
}

pub type NeverCloneUsizeAllocator =
    FromUsizeAllocator<usize, ForwardWrapper, NeverCloneLinkedList<usize>>;
pub type FastCloneUsizeAllocator =
    FromUsizeAllocator<usize, ForwardWrapper, FastCloneLinkedList<usize>>;

#[derive(Clone)]
pub struct FromUsizeAllocator<T: FromUsize, R: Wrapper<T>, S: Stack<usize>> {
    iter: FromUsizeIter<T>,
    recycled: S,
    using: usize,
    _marker: PhantomData<R>,
}

macro_rules! from_usize_allocator_const_new_impl {
    ($contain: ident) => {
        impl<T: FromUsize, R: Wrapper<T>> FromUsizeAllocator<T, R, $contain<usize>> {
            pub const fn new(start: usize) -> Self {
                Self {
                    iter: FromUsizeIter::new(start),
                    recycled: $contain::new(),
                    using: 0,
                    _marker: PhantomData,
                }
            }
        }
    };
}

from_usize_allocator_const_new_impl!(NeverCloneLinkedList);
from_usize_allocator_const_new_impl!(FastCloneLinkedList);

impl<T: FromUsize, R: Wrapper<T>, S: Stack<usize>> FromUsizeAllocator<T, R, S> {
    pub fn alloc(&mut self) -> R::Output {
        self.using += 1;
        if let Some(value) = self.recycled.pop() {
            R::wrapper(T::from_usize(value))
        } else {
            let value = self.iter.next();
            R::wrapper(value)
        }
    }
    pub unsafe fn dealloc(&mut self, value: T) {
        self.using -= 1;
        self.recycled.push(value.into_usize());
    }
    pub const fn using(&self) -> usize {
        self.using
    }
}
