use alloc::{boxed::Box, sync::Arc};

use crate::{
    memory::{allocator::frame, asid, page_table::PageTableEntry},
    syscall::SysError,
    tools::{
        self,
        allocator::{from_usize_allocator::LeakFromUsizeAllocator, TrackerAllocator},
        container::sync_unsafe_cell::SyncUnsafeCell,
        range::URange,
        xasync::{HandlerID, TryR, TryRunFail},
        ForwardWrapper,
    },
    xdebug::PRINT_PAGE_FAULT,
};

use self::{
    handler::{manager::HandlerManager, AsyncHandler, UserAreaHandler},
    sc_manager::SCManager,
};

use super::{address::UserAddr4K, allocator::frame::iter::FrameDataIter, AccessType, PageTable};

pub mod handler;
mod sc_manager;

type HandlerAllocator = LeakFromUsizeAllocator<HandlerID, ForwardWrapper>;

macro_rules! pt {
    ($self: ident) => {
        unsafe { &mut *$self.page_table.get() }
    };
}
/// own by user_space
pub struct MapSegment {
    pub page_table: Arc<SyncUnsafeCell<PageTable>>,
    handlers: HandlerManager,
    sc_manager: SCManager,
    id_allocator: HandlerAllocator,
}

impl MapSegment {
    pub const fn new(page_table: Arc<SyncUnsafeCell<PageTable>>) -> Self {
        Self {
            page_table,
            handlers: HandlerManager::new(),
            sc_manager: SCManager::new(),
            id_allocator: HandlerAllocator::default(),
        }
    }
    /// 范围必须不存在映射 否则 panic
    ///
    /// 返回初始化结果 失败则撤销映射
    pub fn force_push(&mut self, r: URange, h: Box<dyn UserAreaHandler>) -> Result<(), SysError> {
        let pt = pt!(self);
        let h = self.handlers.try_push(r.clone(), h).ok().unwrap();
        let id = self.id_allocator.alloc();
        h.init(id, pt, r.clone()).inspect_err(|_e| self.unmap(r))
    }
    fn release_impl<'a>(
        pt: &'a mut PageTable,
        sc_manager: &'a mut SCManager,
    ) -> impl FnMut(Box<dyn UserAreaHandler>, URange) + 'a {
        move |h, r: URange| {
            let pt = pt as *mut PageTable;
            macro_rules! pt {
                () => {
                    unsafe { &mut *pt }
                };
            }
            let shared_release = |addr| {
                let pte = pt!().try_get_pte_user(addr).unwrap();
                debug_assert!(pte.is_leaf());
                pte.reset();
            };
            let unique_release = |addr| h.unmap_ua(pt!(), addr);
            sc_manager.remove_release(r.clone(), shared_release, unique_release);
            h.unmap(pt!(), r);
        }
    }
    /// 释放存在映射的空间
    pub fn unmap(&mut self, r: URange) {
        let sc_manager = &mut self.sc_manager; // stupid borrow checker
        let release = Self::release_impl(pt!(self), sc_manager);
        self.handlers.remove(r, release);
    }
    pub fn clear(&mut self) {
        let sc_manager = &mut self.sc_manager;
        let release = Self::release_impl(pt!(self), sc_manager);
        self.handlers.clear(release);
        assert!(sc_manager.is_empty());
    }
    pub fn replace(&mut self, r: URange, h: Box<dyn UserAreaHandler>) -> Result<(), SysError> {
        self.unmap(r.clone());
        self.force_push(r, h)
    }
    /// 如果进入 async 状态将 panic
    pub fn force_map(&self, r: URange) -> Result<(), SysError> {
        stack_trace!();
        let pt = pt!(self);
        let h = self.handlers.range_contain(r.clone()).unwrap();
        stack_trace!();
        h.map(pt, r).map_err(|e| match e {
            TryRunFail::Async(_a) => panic!(),
            TryRunFail::Error(e) => e,
        })
    }
    /// 此函数可以向只读映射写入数据 但不能修改只读共享页
    pub fn force_write_range(
        &self,
        r: URange,
        mut data: impl FrameDataIter,
    ) -> Result<(), SysError> {
        stack_trace!();
        let pt = pt!(self);
        self.force_map(r.clone())?;
        for addr in tools::range::ur_iter(r) {
            pt.force_convert_user(addr, |pte| {
                assert!(!pte.shared() || pte.writable());
                let _ = data.write_to(pte.phy_addr().into_ref().as_bytes_array_mut());
            });
        }
        Ok(())
    }
    pub fn page_fault(
        &mut self,
        addr: UserAddr4K,
        access: AccessType,
    ) -> TryR<(), Box<dyn AsyncHandler>> {
        debug_assert!(access.user);
        let h = self
            .handlers
            .get(addr)
            .ok_or(TryRunFail::Error(SysError::EFAULT))?;

        let pt = pt!(self);
        let pte = match pt.try_get_pte_user(addr) {
            None => return h.page_fault(pt, addr, access),
            Some(a) => a,
        };
        if access.exec {
            debug_assert!(!h.executable());
            debug_assert!(!pte.executable());
            return Err(TryRunFail::Error(SysError::EFAULT));
        }
        assert!(access.write);
        if !h.unique_writable() {
            return Err(TryRunFail::Error(SysError::EFAULT));
        }
        assert!(pte.shared());
        if self.sc_manager.try_remove_unique(addr) {
            if PRINT_PAGE_FAULT {
                println!("this shared page is unique");
            }
            pte.clear_shared();
            pte.set_writable();
            return Ok(());
        }
        if PRINT_PAGE_FAULT {
            println!("copy to new page");
        }
        let allocator = &mut frame::defualt_allocator();
        let x = allocator.alloc()?;
        x.ptr()
            .as_usize_array_mut()
            .copy_from_slice(pte.phy_addr().into_ref().as_usize_array());
        if self.sc_manager.remove_ua(addr) {
            if PRINT_PAGE_FAULT {
                println!("release old shared page");
            }
            unsafe { pte.dealloc_by(allocator) };
        }
        *pte = PageTableEntry::new(x.consume().into(), h.map_perm());
        Ok(())
    }
    /// 共享优化 fork
    ///
    /// 发生错误时回退到执行前的状态
    pub fn fork(&mut self) -> Result<Self, SysError> {
        stack_trace!();
        let src = pt!(self);
        let mut dst = PageTable::from_global(asid::alloc_asid())?;
        let allocator = &mut frame::defualt_allocator();
        let mut new_sm = SCManager::new();

        let mut err_1 = Ok(());
        for (r, h) in self.handlers.iter() {
            match h.shared_writable() {
                Some(shared_writable) => {
                    let mut err_2 = Ok(());
                    for (addr, src) in src.valid_pte_iter(r.clone()) {
                        let dst = match dst.get_pte_user(addr, allocator) {
                            Ok(x) => x,
                            Err(e) => {
                                err_1 = Err((r.clone(), e.into()));
                                err_2 = Err(addr);
                                break;
                            }
                        };
                        debug_assert!(!dst.is_valid());
                        let sc = if !src.shared() {
                            src.become_shared(shared_writable);
                            self.sc_manager.insert_clone(addr)
                        } else {
                            debug_assert_eq!(src.writable(), shared_writable);
                            self.sc_manager.clone_ua(addr)
                        };
                        new_sm.insert_by(addr, sc);
                        *dst = *src;
                    }
                    // roll back inner
                    let e_addr = match err_2 {
                        Ok(()) => continue,
                        Err(x) => x,
                    };
                    // error happen
                    for (addr, dst) in dst.valid_pte_iter(r.clone()) {
                        if addr == e_addr {
                            break;
                        }
                        new_sm.remove_ua_result(addr).unwrap_err();
                        dst.reset();
                        if self.sc_manager.try_remove_unique(addr) {
                            src.try_get_pte_user(addr)
                                .unwrap()
                                .become_unique(h.unique_writable());
                        }
                    }
                    break;
                }
                None => match h.copy_map(src, &mut dst, r.clone()) {
                    Ok(()) => (),
                    Err(e) => {
                        err_1 = Err((r, e));
                        break;
                    }
                },
            }
        }
        if err_1.is_ok() {
            let new_ms = MapSegment {
                page_table: Arc::new(SyncUnsafeCell::new(dst)),
                handlers: self.handlers.fork(),
                sc_manager: new_sm,
                id_allocator: self.id_allocator.clone(),
            };
            return Ok(new_ms);
        }
        // 错误回退
        let (rr, e) = err_1.unwrap_err();
        new_sm.check_remove_all();
        for (r, h) in self.handlers.iter() {
            if r == rr {
                break;
            }
            match h.shared_writable() {
                Some(_) => {
                    for (addr, pte) in dst.valid_pte_iter(r.clone()) {
                        assert!(pte.shared());
                        pte.reset();
                        if self.sc_manager.try_remove_unique(addr) {
                            src.try_get_pte_user(addr)
                                .unwrap()
                                .become_unique(h.unique_writable());
                        }
                    }
                }
                None => {
                    h.unmap(&mut dst, r);
                }
            }
        }
        Err(e)
    }
}
