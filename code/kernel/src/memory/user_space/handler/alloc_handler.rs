use core::ops::Range;

use alloc::boxed::Box;

use crate::{
    memory::{
        address::{PageCount, UserAddr, UserAddr4K},
        allocator::frame::{self, iter::NullFrameDataIter},
        page_table::PTEFlags,
        user_space::UserArea,
        PageTable,
    },
    syscall::SysError,
    tools::{
        self,
        xasync::{TryR, TryRunFail},
    },
};

use super::{AsyncHandler, HandlerID, UserAreaHandler};

pub struct GlobalAllocHandler {
    id: HandlerID,
    range: Range<UserAddr4K>,
    perm: PTEFlags,
}

impl UserAreaHandler for GlobalAllocHandler {
    fn id(&self) -> HandlerID {
        self.id
    }
    fn range(&self) -> Range<UserAddr4K> {
        self.range.clone()
    }
    fn perm(&self) -> PTEFlags {
        self.perm
    }
    fn try_map(
        &self,
        pt: &mut PageTable,
        range: Range<UserAddr4K>,
    ) -> TryR<(), Box<dyn AsyncHandler>> {
        stack_trace!();
        let range = tools::range_limit(range, self.range());
        if range.start >= range.end {
            return Ok(());
        }
        pt.map_user_range(
            &UserArea::new(range, self.perm()),
            &mut NullFrameDataIter,
            &mut frame::defualt_allocator(),
        )?;
        Ok(())
    }
    fn try_page_fault(
        &self,
        pt: &mut PageTable,
        addr: UserAddr,
    ) -> TryR<(), Box<dyn AsyncHandler>> {
        stack_trace!();
        let addr = addr.floor();
        if !self.contains(addr) {
            return Err(TryRunFail::Fatal(SysError::EFAULT));
        }
        pt.map_user_addr(
            addr,
            self.perm,
            &mut NullFrameDataIter,
            &mut frame::defualt_allocator(),
        )?;
        Ok(())
    }
    fn unmap(&self, pt: &mut PageTable, range: Range<UserAddr4K>) -> PageCount {
        self.default_unmap(pt, range)
    }
    fn split_l(&mut self, addr: UserAddr4K) -> Box<dyn UserAreaHandler> {
        todo!()
    }
    fn split_r(&mut self, addr: UserAddr4K) -> Box<dyn UserAreaHandler> {
        todo!()
    }
}
