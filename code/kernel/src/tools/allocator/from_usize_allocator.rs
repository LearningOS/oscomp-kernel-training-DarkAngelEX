use core::marker::PhantomData;

use alloc::vec::Vec;

use super::Own;

pub trait FromUsize {
    fn from_usize(num: usize) -> Self;
}

impl FromUsize for usize {
    fn from_usize(num: usize) -> Self {
        num
    }
}

#[macro_export]
macro_rules! from_usize_impl {
    ($ty_name: ident) => {
        impl crate::tools::allocator::from_usize_allocator::FromUsize for $ty_name {
            fn from_usize(num: usize) -> Self {
                Self(num)
            }
        }
    };
}

#[derive(Debug)]
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
pub type UsizeAllocator = FromUsizeAllocator<usize, usize>;

#[derive(Debug)]
pub struct FromUsizeAllocator<T: FromUsize, R: From<T>> {
    iter: FromUsizeIter<T>,
    recycled: Vec<T>,
    using: usize,
    _marker: PhantomData<R>,
}

impl<T: FromUsize, R: From<T>> FromUsizeAllocator<T, R> {
    pub const fn new(start: usize) -> Self {
        Self {
            iter: FromUsizeIter::new(start),
            recycled: Vec::new(),
            using: 0,
            _marker: PhantomData,
        }
    }
    pub fn alloc(&mut self) -> R {
        self.using += 1;
        if let Some(value) = self.recycled.pop() {
            R::from(value)
        } else {
            let value = self.iter.next();
            R::from(value)
        }
    }
    pub unsafe fn dealloc(&mut self, value: T) {
        self.using -= 1;
        self.recycled.push(value);
    }
    pub const fn using(&self) -> usize {
        self.using
    }
}
