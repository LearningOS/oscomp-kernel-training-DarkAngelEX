use crate::fs::File;
use crate::memory::address::UserAddr4K;
use crate::memory::allocator::frame;
use crate::memory::map_segment::handler::{AsyncHandler, FileAsyncHandler, UserAreaHandler};
use crate::memory::page_table::PTEFlags;
use crate::memory::{AccessType, PageTable};
use crate::syscall::SysError;
use crate::tools::range::URange;
use crate::tools::xasync::{HandlerID, TryR, TryRunFail};
use alloc::boxed::Box;
use alloc::sync::Arc;

#[derive(Clone)]
pub struct MmapHandler {
    id: Option<HandlerID>,
    file: Option<Arc<dyn File>>,
    addr: UserAddr4K, // MMAP开始地址
    offset: usize,    // 文件偏移量
    perm: PTEFlags,
    shared: bool,
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
            id: None,
            file,
            addr,
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
        stack_trace!();
        if range.start >= range.end {
            return Ok(());
        }
        let file = match self.file.as_ref() {
            None => return self.default_map(pt, range),
            Some(file) => file.clone(),
        };
        return Err(TryRunFail::Async(Box::new(FileAsyncHandler::new(
            self.id(),
            self.perm(),
            self.addr,
            self.offset,
            file,
        ))));
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
        access
            .check(self.perm())
            .map_err(|_| TryRunFail::Error(SysError::EFAULT))?;
        let file = match self.file.as_ref() {
            None => return self.default_page_fault(pt, addr, access),
            Some(file) => file.clone(),
        };
        return Err(TryRunFail::Async(Box::new(FileAsyncHandler::new(
            self.id(),
            self.perm(),
            self.addr,
            self.offset,
            file,
        ))));
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
