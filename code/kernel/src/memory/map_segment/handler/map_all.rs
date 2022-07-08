use alloc::boxed::Box;

use crate::{
    memory::{
        address::UserAddr4K, asid::Asid, page_table::PTEFlags, user_space::AccessType, PageTable,
    },
    syscall::SysError,
    tools::{
        range::URange,
        xasync::{TryR, TryRunFail},
        DynDropRun,
    },
};

use super::{base::HandlerBase, AsyncHandler, HandlerID, UserAreaHandler};

#[derive(Clone)]
pub struct MapAllHandlerSpec {
    id: HandlerID,
    perm: PTEFlags,
}
/// init 调用时一次性将范围内的空间全部分配
#[derive(Clone)]
pub struct MapAllHandler {
    spec: MapAllHandlerSpec,
    base: HandlerBase,
}

impl MapAllHandler {
    pub fn raw_new(perm: PTEFlags) -> Self {
        Self {
            spec: MapAllHandlerSpec {
                id: HandlerID::invalid(),
                perm,
            },
            base: HandlerBase::new(),
        }
    }
    pub fn box_new(perm: PTEFlags) -> Box<dyn UserAreaHandler> {
        Box::new(Self::raw_new(perm))
    }
    pub fn set_id(&mut self, id: HandlerID) {
        self.spec.id = id;
    }
    pub fn box_clone_spec(&self) -> Self {
        Self {
            spec: self.spec.clone(),
            base: HandlerBase::new(),
        }
    }
}

impl UserAreaHandler for MapAllHandler {
    fn id(&self) -> HandlerID {
        debug_assert_ne!(self.spec.id, HandlerID::invalid());
        self.spec.id
    }
    fn perm(&self) -> PTEFlags {
        self.spec.perm
    }
    fn base(&self) -> &HandlerBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut HandlerBase {
        &mut self.base
    }
    fn init(&mut self, id: HandlerID, pt: &mut PageTable, all: URange) -> Result<(), SysError> {
        stack_trace!();
        self.set_id(id);
        self.map(pt, all).map_err(|e| match e {
            TryRunFail::Async(_) => panic!(),
            TryRunFail::Error(e) => e,
        })
    }
    fn modify_perm(&mut self, perm: PTEFlags) {
        self.spec.perm = perm;
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
    ) -> TryR<DynDropRun<(UserAddr4K, Asid)>, Box<dyn AsyncHandler>> {
        stack_trace!();
        self.default_page_fault(pt, addr, access)
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
    fn box_clone_spec(&self) -> Box<dyn UserAreaHandler> {
        Box::new(self.box_clone_spec())
    }
}
