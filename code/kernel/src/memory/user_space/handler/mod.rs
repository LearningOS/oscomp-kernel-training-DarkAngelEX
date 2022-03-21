pub mod alloc_handler;

use core::ops::{Range, RangeBounds};

use crate::{
    memory::{
        address::{PageCount, UserAddr, UserAddr4K},
        page_table::PTEFlags,
        PageTable,
    },
    tools::Async,
};

pub trait UserAreaHandler {
    fn range(&self) -> Range<UserAddr4K>;
    fn contains(&self, value: UserAddr4K) -> bool {
        self.range().contains(&value)
    }
    fn perm(&self) -> PTEFlags;
    fn map(&self, pt: &mut PageTable, range: impl RangeBounds<UserAddr4K>)
        -> Async<Result<(), ()>>;
    fn unmap(&self, pt: &mut PageTable, range: impl RangeBounds<UserAddr4K>) -> Async<PageCount>;
    fn page_fault(&self, pt: &mut PageTable, addr: UserAddr) -> Async<Result<(), ()>>;
    fn writable(&self) -> bool {
        self.perm().contains(PTEFlags::W)
    }
    fn execable(&self) -> bool {
        self.perm().contains(PTEFlags::X)
    }
}
