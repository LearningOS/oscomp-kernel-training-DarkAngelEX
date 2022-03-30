use core::marker::PhantomData;

use crate::tools::{
    container::{
        fast_clone_linked_list::FastCloneLinkedList, never_clone_linked_list::NeverCloneLinkedList,
        LeakStack, Stack,
    },
    ForwardWrapper, Wrapper,
};

pub trait FromUsize {
    fn from_usize(num: usize) -> Self;
    fn to_usize(&self) -> usize;
}

impl FromUsize for usize {
    fn from_usize(num: usize) -> Self {
        num
    }
    fn to_usize(&self) -> usize {
        *self
    }
}

#[macro_export]
macro_rules! from_usize_impl {
    ($ty_name: ident) => {
        impl From<usize> for $ty_name {
            fn from(num: usize) -> Self {
                Self(num)
            }
        }
        impl crate::tools::allocator::from_usize_allocator::FromUsize for $ty_name {
            fn from_usize(num: usize) -> Self {
                Self(num)
            }
            fn to_usize(&self) -> usize {
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
    pub const fn set_next(&mut self, v: usize) {
        self.next = v;
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

/// FU: FromUsize
pub type LeakFromUsizeAllocator<T, R> = FromUsizeAllocator<T, R, LeakStack>;

impl<T: FromUsize, R: Wrapper<T>, S: Stack<usize> + ~const Default> const Default
    for FromUsizeAllocator<T, R, S>
{
    fn default() -> Self {
        Self {
            iter: FromUsizeIter::new(0),
            recycled: S::default(),
            using: 0,
            _marker: PhantomData,
        }
    }
}

impl<T: FromUsize, R: Wrapper<T>, S: Stack<usize>> FromUsizeAllocator<T, R, S> {
    // this will only be used after default()
    pub const fn start(mut self, start: usize) -> Self {
        self.iter.set_next(start);
        self
    }
    pub fn alloc(&mut self) -> R::Output {
        self.using += 1;
        if let Some(value) = self.recycled.pop() {
            R::wrapper(T::from_usize(value))
        } else {
            let value = self.iter.next();
            R::wrapper(value)
        }
    }
    /// It must be ensured that the value is released only once.
    pub unsafe fn dealloc(&mut self, value: T) {
        self.using -= 1;
        self.recycled.push(value.to_usize());
    }
    pub const fn using(&self) -> usize {
        self.using
    }
}
