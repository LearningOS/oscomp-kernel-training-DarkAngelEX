
#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct Tid(usize);

from_usize_impl!(Tid);

// pub type TidAllocator = NeverCloneUsizeAllocator;

