use core::ops::{Range, RangeBounds};

use alloc::boxed::Box;

use crate::{
    memory::{
        address::{PageCount, UserAddr, UserAddr4K},
        allocator::frame::{self, iter::NullFrameDataIter},
        page_table::PTEFlags,
        user_space::UserArea,
        PageTable,
    },
    tools::{self, Async},
};

use super::UserAreaHandler;

pub struct GlobalAllocHandler {
    range: Range<UserAddr4K>,
    perm: PTEFlags,
}

impl UserAreaHandler for GlobalAllocHandler {
    fn range(&self) -> Range<UserAddr4K> {
        self.range.clone()
    }
    fn perm(&self) -> PTEFlags {
        self.perm
    }
    fn map(
        &self,
        pt: &mut PageTable,
        range: impl RangeBounds<UserAddr4K>,
    ) -> Async<Result<(), ()>> {
        let range = tools::range_limit(range, self.range());
        if range.start >= range.end {
            return Box::pin(async move { Ok(()) });
        }
        let ret = pt
            .map_user_range(
                &UserArea::new(range, self.perm()),
                &mut NullFrameDataIter,
                &mut frame::defualt_allocator(),
            )
            .map_err(|_e| ());
        Box::pin(async move { ret })
    }
    fn unmap(&self, pt: &mut PageTable, range: impl RangeBounds<UserAddr4K>) -> Async<PageCount> {
        let range = tools::range_limit(range, self.range());
        if range.start >= range.end {
            return Box::pin(async move { PageCount::from_usize(0) });
        }
        let ret = pt.unmap_user_range_lazy(
            &UserArea::new(range, self.perm()),
            &mut frame::defualt_allocator(),
        );
        Box::pin(async move { ret })
    }
    fn page_fault(&self, pt: &mut PageTable, addr: UserAddr) -> Async<Result<(), ()>> {
        let addr = addr.floor();
        if !self.contains(addr) {
            return Box::pin(async move { Err(()) });
        }
        let ret = pt
            .map_user_addr(
                addr,
                self.perm,
                &mut NullFrameDataIter,
                &mut frame::defualt_allocator(),
            )
            .map_err(|_e| ());
        Box::pin(async move { ret })
    }
}
