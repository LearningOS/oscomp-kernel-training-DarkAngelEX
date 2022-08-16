use alloc::{boxed::Box, sync::Arc};
use ftl_util::{error::SysR, faster};
use vfs::File;

use crate::{
    config::PAGE_SIZE,
    memory::{
        address::{UserAddr, UserAddr4K},
        allocator::frame::{global::FrameTracker, FrameAllocator},
        asid::Asid,
        map_segment::{
            handler::{AsyncHandler, FileAsyncHandler, UserAreaHandler},
            shared::SharedCounter,
            zero_copy::{self, SharePage, ZeroCopy},
        },
        page_table::PTEFlags,
        {AccessType, PageTable},
    },
    sync::mutex::SpinLock,
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
    zero_copy: Option<Arc<SpinLock<ZeroCopy>>>,
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
        let zero_copy = if let Some(Ok(file)) = file.as_ref().map(|f| f.vfs_file()) {
            let (dev, ino) = file.dev_ino();
            Some(zero_copy::get_zero_copy(dev, ino))
        } else {
            None
        };
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
                zero_copy,
            },
            base: HandlerBase::new(),
        })
    }

    fn get_offset(&self, addr: UserAddr4K) -> usize {
        self.spec
            .offset
            .wrapping_add(addr.into_usize())
            .wrapping_sub(self.spec.addr.into_usize())
    }

    fn fast_load_data(
        &self,
        file: &dyn File,
        addr: UserAddr4K,
        dst: &mut [usize; 512],
    ) -> SysR<()> {
        let frame_buf: &mut [u8; 4096] = unsafe { core::mem::transmute(dst) };
        let addr_uz = addr.into_usize();
        let start_uz = self.spec.addr.into_usize();
        let n = if addr_uz < start_uz {
            let start = start_uz - addr_uz; // 未对齐偏移量
            debug_assert!(start < PAGE_SIZE);
            let read = file.read_at_fast(self.spec.offset, &mut frame_buf[start..])?;
            start + read.min(self.spec.fill_size)
        } else {
            let offset = self.spec.offset + addr_uz - start_uz;
            if offset < self.spec.offset + self.spec.fill_size {
                let read = file.read_at_fast(offset, frame_buf)?;
                read.min(self.spec.offset + self.spec.fill_size - offset)
            } else {
                0 // 填充0
            }
        };
        frame_buf[n..].fill(0);
        Ok(())
    }

    fn map_fast(
        &self,
        file: &dyn File,
        pt: &mut PageTable,
        range: URange,
        allocator: &mut dyn FrameAllocator,
        cur: &mut UserAddr4K, // 慢速路径直接从这个地址开始
    ) -> SysR<()> {
        if !file.can_read_offset() {
            return Err(SysError::EACCES);
        }
        let alloc = unsafe { &mut *(allocator as *mut _) };
        let zc = self.spec.zero_copy.as_ref().unwrap();
        let start = range.start;
        let offset = self.get_offset(start).wrapping_sub(start.into_usize());
        for r in pt.each_pte_iter(range, alloc) {
            let (addr, pte) = r?;
            *cur = addr;
            if pte.is_valid() {
                continue;
            }
            debug_assert!(addr >= self.spec.addr.floor());
            let frame: FrameTracker = allocator.alloc()?;
            let this_off = offset.wrapping_add(addr.into_usize());
            let page = zc.lock().get(this_off).cloned();
            if let Some(page) = page {
                faster::page_copy(frame.data().as_usize_array_mut(), page.as_usize_array());
            } else {
                self.fast_load_data(file, addr, frame.data().as_usize_array_mut())?;
                let sp = allocator.alloc()?;
                faster::page_copy(
                    sp.data().as_usize_array_mut(),
                    frame.data().as_usize_array_mut(),
                );
                zc.lock()
                    .insert(this_off, SharePage::new(SharedCounter::new(), sp.consume()));
            }
            pte.alloc_by_frame(self.perm(), frame.consume());
        }
        Ok(())
    }

    fn page_fault_fast(
        &self,
        file: &dyn File,
        pt: &mut PageTable,
        addr: UserAddr4K,
        allocator: &mut dyn FrameAllocator,
    ) -> SysR<()> {
        stack_trace!();
        if !file.can_read_offset() {
            return Err(SysError::EACCES);
        }
        let pte = pt.get_pte_user(addr, allocator)?;
        if pte.is_valid() {
            return Ok(());
        }
        // 调用这个函数的时候 try_rd_only_shared 已经失败了
        debug_assert!(addr >= self.spec.addr.floor());
        let frame: FrameTracker = allocator.alloc()?;
        self.fast_load_data(file, addr, frame.data().as_usize_array_mut())?;
        let zc = self.spec.zero_copy.as_ref().unwrap();
        let sp = allocator.alloc()?;
        let offset = self.get_offset(addr);
        faster::page_copy(
            sp.data().as_usize_array_mut(),
            frame.data().as_usize_array_mut(),
        );
        zc.lock()
            .insert(offset, SharePage::new(SharedCounter::new(), sp.consume()));

        pte.alloc_by_frame(self.perm(), frame.consume());
        Ok(())
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
    fn exec_reuse(&self) -> bool {
        self.spec.init_program && !self.unique_writable()
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
    fn init_no_release(
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
        self.spec.init_program = false;
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

        // =================
        let mut cur = range.start;
        match self.map_fast(&*file, pt, range, allocator, &mut cur) {
            Ok(()) => return Ok(()),
            Err(SysError::EAGAIN) => (),
            Err(e) => return Err(TryRunFail::Error(e)),
        }

        // =================

        Err(TryRunFail::Async(Box::new(FileAsyncHandler::new(
            self.id(),
            self.perm(),
            self.spec.addr,
            self.spec.offset,
            self.spec.fill_size,
            file,
            cur,
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

        match self.page_fault_fast(&*file, pt, addr, allocator) {
            Ok(()) => return Ok(pt.flush_va_asid_fn(addr)),
            Err(SysError::EAGAIN) => (),
            Err(e) => return Err(TryRunFail::Error(e)),
        }

        Err(TryRunFail::Async(Box::new(FileAsyncHandler::new(
            self.id(),
            self.perm(),
            self.spec.addr,
            self.spec.offset,
            self.spec.fill_size,
            file,
            addr,
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
    fn try_rd_only_shared(
        &self,
        addr: UserAddr4K,
        allocator: &mut dyn FrameAllocator,
    ) -> Option<SharePage> {
        if let Some(zc) = self.spec.zero_copy.as_ref() {
            if unsafe { zc.unsafe_get().is_empty() } {
                return None;
            }
            debug_assert!(self.spec.file.is_some());
            let offset = self.get_offset(addr);
            if let Some(page) = zc.lock().get(offset).cloned() {
                return Some(page);
            }
            let file = self.spec.file.as_ref().unwrap().vfs_file().ok()?;
            let frame = allocator.alloc().ok()?;
            self.fast_load_data(file, addr, frame.data().as_usize_array_mut())
                .ok()?;
            let (s0, s1) = SharedCounter::new_dup();
            let s0 = SharePage::new(s0, frame.data());
            let s1 = SharePage::new(s1, frame.consume());
            zc.lock().insert(offset, s0);
            Some(s1)
        } else {
            None
        }
    }
}
