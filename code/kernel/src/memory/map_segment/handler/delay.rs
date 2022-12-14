use alloc::boxed::Box;
use ftl_util::error::SysR;

use crate::{
    memory::{
        address::UserAddr4K, allocator::frame::FrameAllocator, asid::Asid, page_table::PTEFlags,
        user_space::AccessType, PageTable,
    },
    tools::{range::URange, xasync::TryR, DynDropRun},
};

use super::{base::HandlerBase, map_all::MapAllHandler, AsyncHandler, HandlerID, UserAreaHandler};

/// 和 UniqueHandler 的唯一区别是 init_map 只映射参数区域
///
/// 保证初始化成功
#[derive(Clone)]
pub struct DelayHandler {
    inner: MapAllHandler,
}

impl DelayHandler {
    pub fn box_new(perm: PTEFlags) -> Box<dyn UserAreaHandler> {
        Box::new(Self {
            inner: MapAllHandler::raw_new(perm),
        })
    }
}

impl UserAreaHandler for DelayHandler {
    fn id(&self) -> HandlerID {
        self.inner.id()
    }
    fn perm(&self) -> PTEFlags {
        self.inner.perm()
    }
    fn base(&self) -> &HandlerBase {
        self.inner.base()
    }
    fn base_mut(&mut self) -> &mut HandlerBase {
        self.inner.base_mut()
    }
    /// 唯一的区别是放弃初始化
    fn init(
        &mut self,
        id: HandlerID,
        _pt: &mut PageTable,
        _all: URange,
        _allocator: &mut dyn FrameAllocator,
    ) -> SysR<()> {
        self.inner.set_id(id);
        Ok(())
    }
    fn modify_perm(&mut self, perm: PTEFlags) {
        self.inner.modify_perm(perm)
    }
    fn map_spec(
        &self,
        pt: &mut PageTable,
        range: URange,
        allocator: &mut dyn FrameAllocator,
    ) -> TryR<(), Box<dyn AsyncHandler>> {
        stack_trace!();
        self.inner.map_spec(pt, range, allocator)
    }
    fn copy_map_spec(
        &self,
        src: &mut PageTable,
        dst: &mut PageTable,
        r: URange,
        allocator: &mut dyn FrameAllocator,
    ) -> SysR<()> {
        stack_trace!();
        self.inner.copy_map_spec(src, dst, r, allocator)
    }
    fn page_fault_spec(
        &self,
        pt: &mut PageTable,
        addr: UserAddr4K,
        access: AccessType,
        allocator: &mut dyn FrameAllocator,
    ) -> TryR<DynDropRun<(UserAddr4K, Asid)>, Box<dyn AsyncHandler>> {
        stack_trace!();
        self.inner.page_fault_spec(pt, addr, access, allocator)
    }
    fn unmap_spec(&self, pt: &mut PageTable, range: URange, allocator: &mut dyn FrameAllocator) {
        self.inner.unmap_spec(pt, range, allocator)
    }
    fn unmap_ua_spec(
        &self,
        pt: &mut PageTable,
        addr: UserAddr4K,
        allocator: &mut dyn FrameAllocator,
    ) {
        self.inner.unmap_ua_spec(pt, addr, allocator)
    }
    fn box_clone(&self) -> Box<dyn UserAreaHandler> {
        Box::new(self.clone())
    }
    fn box_clone_spec(&self) -> Box<dyn UserAreaHandler> {
        Box::new(self.inner.box_clone_spec())
    }
}
