use crate::fs::File;
use crate::memory::address::UserAddr4K;
use crate::memory::map_segment::handler::{AsyncHandler, UserAreaHandler};
use crate::memory::page_table::PTEFlags;
use crate::memory::{AccessType, PageTable};
use crate::syscall::SysError;
use crate::tools::range::URange;
use crate::tools::xasync::{HandlerID, TryR};
use alloc::boxed::Box;
use alloc::sync::Arc;

#[derive(Clone)]
pub struct MmapHandler {
    id: Option<HandlerID>,
    file: Option<Arc<dyn File>>,
    offset: usize,
    perm: PTEFlags,
    shared: bool,
}

impl MmapHandler {
    pub fn box_new(
        file: Option<Arc<dyn File>>,
        offset: usize,
        perm: PTEFlags,
        shared: bool,
    ) -> Box<dyn UserAreaHandler> {
        Box::new(MmapHandler {
            id: None,
            file,
            offset,
            perm,
            shared,
        })
    }
}

impl UserAreaHandler for MmapHandler {
    fn id(&self) -> HandlerID {
        self.id.unwrap()
    }
    fn perm(&self) -> PTEFlags {
        self.perm
    }
    fn shared_always(&self) -> bool {
        self.shared
    }
    fn init(&mut self, id: HandlerID, _pt: &mut PageTable, _all: URange) -> Result<(), SysError> {
        self.id = Some(id);
        Ok(())
    }
    fn max_perm(&self) -> PTEFlags {
        match &self.file {
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
        self.perm = perm;
    }
    fn map(&self, pt: &mut PageTable, range: URange) -> TryR<(), Box<dyn AsyncHandler>> {
        match self.file {
            None => self.default_map(pt, range),
            Some(_) => todo!(),
        }
    }
    fn copy_map(
        &self,
        src: &mut PageTable,
        dst: &mut PageTable,
        r: URange,
    ) -> Result<(), SysError> {
        self.default_copy_map(src, dst, r)
    }
    fn page_fault(
        &self,
        pt: &mut PageTable,
        addr: UserAddr4K,
        access: AccessType,
    ) -> TryR<(), Box<dyn AsyncHandler>> {
        match self.file {
            None => self.default_page_fault(pt, addr, access),
            Some(_) => todo!(),
        }
    }
    fn unmap(&self, pt: &mut PageTable, range: URange) {
        self.default_unmap(pt, range)
    }
    fn unmap_ua(&self, pt: &mut PageTable, addr: UserAddr4K) {
        self.default_unmap_ua(pt, addr)
    }
    fn box_clone(&self) -> Box<dyn UserAreaHandler> {
        Box::new(self.clone())
    }
}
