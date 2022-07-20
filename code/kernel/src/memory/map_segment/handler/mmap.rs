use alloc::{boxed::Box, sync::Arc};
use ftl_util::error::SysR;
use vfs::File;

use crate::{
    memory::{
        address::UserAddr4K,
        asid::Asid,
        map_segment::handler::{AsyncHandler, FileAsyncHandler, UserAreaHandler},
        page_table::PTEFlags,
        {AccessType, PageTable},
    },
    syscall::SysError,
    tools::{
        range::URange,
        xasync::{HandlerID, TryR, TryRunFail},
        DynDropRun,
    },
};

use super::base::HandlerBase;

#[derive(Clone)]
struct MmapHandlerSpec {
    id: Option<HandlerID>,
    file: Option<Arc<dyn File>>,
    addr: UserAddr4K, // MMAP开始地址
    offset: usize,    // 文件偏移量
    perm: PTEFlags,
    shared: bool,
}
#[derive(Clone)]
pub struct MmapHandler {
    spec: MmapHandlerSpec,
    base: HandlerBase,
}

impl MmapHandler {
    pub fn box_new(
        file: Option<Arc<dyn File>>,
        addr: UserAddr4K,
        offset: usize,
        perm: PTEFlags,
        shared: bool,
    ) -> Box<dyn UserAreaHandler> {
        Box::new(MmapHandler {
            spec: MmapHandlerSpec {
                id: None,
                file,
                addr,
                offset,
                perm,
                shared,
            },
            base: HandlerBase::new(),
        })
    }
}

impl UserAreaHandler for MmapHandler {
    fn id(&self) -> HandlerID {
        self.spec.id.unwrap()
    }
    fn perm(&self) -> PTEFlags {
        self.spec.perm
    }
    fn shared_always(&self) -> bool {
        self.spec.shared
    }
    fn base(&self) -> &HandlerBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut HandlerBase {
        &mut self.base
    }
    fn init(&mut self, id: HandlerID, _pt: &mut PageTable, _all: URange) -> SysR<()> {
        self.spec.id = Some(id);
        Ok(())
    }
    fn max_perm(&self) -> PTEFlags {
        match &self.spec.file {
            None => PTEFlags::R | PTEFlags::W | PTEFlags::X | PTEFlags::U,
            Some(f) => {
                let mut perm = PTEFlags::U;
                if f.readable() {
                    perm |= PTEFlags::R | PTEFlags::X;
                }
                if f.writable() {
                    perm |= PTEFlags::W;
                }
                perm
            }
        }
    }
    fn modify_perm(&mut self, perm: PTEFlags) {
        self.spec.perm = perm;
    }
    fn map_spec(&self, pt: &mut PageTable, range: URange) -> TryR<(), Box<dyn AsyncHandler>> {
        stack_trace!();
        if range.start >= range.end {
            return Ok(());
        }
        let file = match self.spec.file.as_ref() {
            None => return self.default_map_spec(pt, range),
            Some(file) => file.clone(),
        };
        return Err(TryRunFail::Async(Box::new(FileAsyncHandler::new(
            self.id(),
            self.perm(),
            self.spec.addr,
            self.spec.offset,
            file,
        ))));
    }
    fn copy_map_spec(&self, src: &mut PageTable, dst: &mut PageTable, r: URange) -> SysR<()> {
        self.default_copy_map_spec(src, dst, r)
    }
    fn page_fault_spec(
        &self,
        pt: &mut PageTable,
        addr: UserAddr4K,
        access: AccessType,
    ) -> TryR<DynDropRun<(UserAddr4K, Asid)>, Box<dyn AsyncHandler>> {
        access
            .check(self.perm())
            .map_err(|_| TryRunFail::Error(SysError::EFAULT))?;
        let file = match self.spec.file.as_ref() {
            None => return self.default_page_fault_spec(pt, addr, access),
            Some(file) => file.clone(),
        };
        return Err(TryRunFail::Async(Box::new(FileAsyncHandler::new(
            self.id(),
            self.perm(),
            self.spec.addr,
            self.spec.offset,
            file,
        ))));
    }
    fn unmap_spec(&self, pt: &mut PageTable, range: URange) {
        self.default_unmap_spec(pt, range)
    }
    fn unmap_ua_spec(&self, pt: &mut PageTable, addr: UserAddr4K) {
        self.default_unmap_ua_spec(pt, addr)
    }
    fn box_clone(&self) -> Box<dyn UserAreaHandler> {
        Box::new(self.clone())
    }
    fn box_clone_spec(&self) -> Box<dyn UserAreaHandler> {
        Box::new(Self {
            spec: self.spec.clone(),
            base: HandlerBase::new(),
        })
    }
}
