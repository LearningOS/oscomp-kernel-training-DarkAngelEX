use alloc::boxed::Box;

use crate::{
    memory::{
        address::UserAddr4K,
        allocator::frame,
        page_table::{PTEFlags, PageTableEntry},
        user_space::AccessType,
        PageTable,
    },
    syscall::SysError,
    tools::{
        allocator::TrackerAllocator,
        range::URange,
        xasync::{TryR, TryRunFail},
    },
};

use super::{AsyncHandler, HandlerID, UserAreaHandler};

/// init 调用时一次性将范围内的空间全部分配
#[derive(Clone)]
pub struct MapAllHandler {
    id: HandlerID,
    perm: PTEFlags,
}

impl MapAllHandler {
    pub fn raw_new(perm: PTEFlags) -> Self {
        Self {
            id: HandlerID::invalid(),
            perm,
        }
    }
    pub fn box_new(perm: PTEFlags) -> Box<dyn UserAreaHandler> {
        Box::new(Self::raw_new(perm))
    }
    pub fn set_id(&mut self, id: HandlerID) {
        self.id = id;
    }
}

impl UserAreaHandler for MapAllHandler {
    fn id(&self) -> HandlerID {
        debug_assert_ne!(self.id, HandlerID::invalid());
        self.id
    }
    fn perm(&self) -> PTEFlags {
        self.perm
    }
    fn init(&mut self, id: HandlerID, pt: &mut PageTable, all: URange) -> Result<(), SysError> {
        stack_trace!();
        self.set_id(id);
        self.map(pt, all).map_err(|e| match e {
            TryRunFail::Async(_) => panic!(),
            TryRunFail::Error(e) => e,
        })
    }
    fn map(&self, pt: &mut PageTable, range: URange) -> TryR<(), Box<dyn AsyncHandler>> {
        self.default_map(pt, range)
    }
    fn copy_map(
        &self,
        src: &mut PageTable,
        dst: &mut PageTable,
        r: URange,
    ) -> Result<(), SysError> {
        stack_trace!();
        self.default_copy_map(src, dst, r)
    }
    fn page_fault(
        &self,
        pt: &mut PageTable,
        addr: UserAddr4K,
        access: AccessType,
    ) -> TryR<(), Box<dyn AsyncHandler>> {
        stack_trace!();
        access
            .check(self.perm)
            .map_err(|_| TryRunFail::Error(SysError::EFAULT))?;
        // 可能同时进入的另一个线程已经处理了这个页错误
        pt.force_map_user(
            addr,
            || {
                let pa = frame::defualt_allocator().alloc()?.consume();
                Ok(PageTableEntry::new(pa.into(), self.map_perm()))
            },
            &mut frame::defualt_allocator(),
        )?;
        Ok(())
    }
    fn unmap(&self, pt: &mut PageTable, range: URange) {
        stack_trace!();
        self.default_unmap(pt, range)
    }
    fn unmap_ua(&self, pt: &mut PageTable, addr: UserAddr4K) {
        stack_trace!();
        self.default_unmap_ua(pt, addr)
    }
    fn box_clone(&self) -> Box<dyn UserAreaHandler> {
        Box::new(self.clone())
    }
}
