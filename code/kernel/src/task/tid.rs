use crate::{from_usize_impl, tools::allocator::from_usize_allocator::FromUsizeAllocator};

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct Tid(usize);

from_usize_impl!(Tid);

impl Tid {
    pub fn get_usize(&self) -> usize {
        self.0
    }
}

pub type TidAllocator = FromUsizeAllocator<Tid, Tid>;
