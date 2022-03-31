use alloc::boxed::Box;

use crate::{
    memory::{address::UserAddr4K, page_table::PTEFlags, user_space::AccessType, PageTable},
    syscall::SysError,
    tools::{range::URange, xasync::TryR},
};

use super::{map_all::MapAllHandler, AsyncHandler, HandlerID, UserAreaHandler};

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
    /// 唯一的区别是放弃初始化
    fn init(&mut self, id: HandlerID, _pt: &mut PageTable, _all: URange) -> Result<(), SysError> {
        self.inner.set_id(id);
        Ok(())
    }
    fn modify_perm(&mut self, perm: PTEFlags) {
        self.inner.modify_perm(perm)
    }
    fn map(&self, pt: &mut PageTable, range: URange) -> TryR<(), Box<dyn AsyncHandler>> {
        stack_trace!();
        self.inner.map(pt, range)
    }
    fn copy_map(
        &self,
        src: &mut PageTable,
        dst: &mut PageTable,
        r: URange,
    ) -> Result<(), SysError> {
        stack_trace!();
        self.inner.copy_map(src, dst, r)
    }
    fn page_fault(
        &self,
        pt: &mut PageTable,
        addr: UserAddr4K,
        access: AccessType,
    ) -> TryR<(), Box<dyn AsyncHandler>> {
        stack_trace!();
        self.inner.page_fault(pt, addr, access)
    }
    fn unmap(&self, pt: &mut PageTable, range: URange) {
        self.inner.unmap(pt, range)
    }
    fn unmap_ua(&self, pt: &mut PageTable, addr: UserAddr4K) {
        self.inner.unmap_ua(pt, addr)
    }
    fn box_clone(&self) -> Box<dyn UserAreaHandler> {
        Box::new(self.clone())
    }
}
