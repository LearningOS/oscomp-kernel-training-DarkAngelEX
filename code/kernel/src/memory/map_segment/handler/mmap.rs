use alloc::{boxed::Box, sync::Arc};
use ftl_util::error::SysR;
use vfs::File;

use crate::{
    memory::{
        address::{UserAddr, UserAddr4K},
        allocator::frame::FrameAllocator,
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
    addr: UserAddr<u8>, // 文件MMAP开始地址, 未对齐则在前面填充0
    offset: usize,      // 文件偏移量
    fill_size: usize,   // 文件长度, 超过的填0, 全部映射则填usize::MAX
    perm: PTEFlags,
    shared: bool,
    init_program: bool,
}

#[derive(Clone)]
pub struct MmapHandler {
    spec: MmapHandlerSpec,
    base: HandlerBase,
}

impl MmapHandler {
    pub fn box_new(
        file: Option<Arc<dyn File>>,
        addr: UserAddr<u8>,
        offset: usize,
        fill_size: usize,
        perm: PTEFlags,
        shared: bool,
        init_program: bool,
    ) -> Box<dyn UserAreaHandler> {
        Box::new(MmapHandler {
            spec: MmapHandlerSpec {
                id: None,
                file,
                addr,
                offset,
                fill_size,
                perm,
                shared,
                init_program,
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
    fn is_init_program(&self) -> bool {
        self.spec.init_program
    }
    fn init(
        &mut self,
        id: HandlerID,
        _pt: &mut PageTable,
        _all: URange,
        _allocator: &mut dyn FrameAllocator,
    ) -> SysR<()> {
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
    fn map_spec(
        &self,
        pt: &mut PageTable,
        range: URange,
        allocator: &mut dyn FrameAllocator,
    ) -> TryR<(), Box<dyn AsyncHandler>> {
        stack_trace!();
        if range.start >= range.end {
            return Ok(());
        }
        let file = match self.spec.file.as_ref() {
            None => return self.default_map_spec(pt, range, allocator),
            Some(file) => file.clone(),
        };
        Err(TryRunFail::Async(Box::new(FileAsyncHandler::new(
            self.id(),
            self.perm(),
            self.spec.addr,
            self.spec.offset,
            self.spec.fill_size,
            file,
        ))))
    }
    fn copy_map_spec(
        &self,
        src: &mut PageTable,
        dst: &mut PageTable,
        r: URange,
        allocator: &mut dyn FrameAllocator,
    ) -> SysR<()> {
        self.default_copy_map_spec(src, dst, r, allocator)
    }
    fn page_fault_spec(
        &self,
        pt: &mut PageTable,
        addr: UserAddr4K,
        access: AccessType,
        allocator: &mut dyn FrameAllocator,
    ) -> TryR<DynDropRun<(UserAddr4K, Asid)>, Box<dyn AsyncHandler>> {
        access
            .check(self.perm())
            .map_err(|_| TryRunFail::Error(SysError::EFAULT))?;
        let file = match self.spec.file.as_ref() {
            None => return self.default_page_fault_spec(pt, addr, access, allocator),
            Some(file) => file.clone(),
        };
        Err(TryRunFail::Async(Box::new(FileAsyncHandler::new(
            self.id(),
            self.perm(),
            self.spec.addr,
            self.spec.offset,
            self.spec.fill_size,
            file,
        ))))
    }
    fn unmap_spec(&self, pt: &mut PageTable, range: URange, allocator: &mut dyn FrameAllocator) {
        self.default_unmap_spec(pt, range, allocator)
    }
    fn unmap_ua_spec(
        &self,
        pt: &mut PageTable,
        addr: UserAddr4K,
        allocator: &mut dyn FrameAllocator,
    ) {
        self.default_unmap_ua_spec(pt, addr, allocator)
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
