use alloc::{
    boxed::Box,
    sync::{Arc, Weak},
};
use ftl_util::{error::SysR, faster};

use crate::{
    futex::{FutexSet, OwnFutex},
    memory::{allocator::frame, asid, page_table::PageTableEntry},
    syscall::SysError,
    tools::{
        self,
        allocator::from_usize_allocator::LeakFromUsizeAllocator,
        container::sync_unsafe_cell::SyncUnsafeCell,
        range::URange,
        xasync::{HandlerID, TryR, TryRunFail},
        DynDropRun, ForwardWrapper,
    },
    xdebug::PRINT_PAGE_FAULT,
};

use self::{
    handler::{manager::HandlerManager, AsyncHandler, UserAreaHandler},
    prediect::Predicter,
    sc_manager::SCManager,
};

use super::{
    address::{PageCount, UserAddr, UserAddr4K},
    allocator::frame::{iter::FrameDataIter, FrameAllocator},
    asid::Asid,
    AccessType, PTEFlags, PageTable,
};

pub mod handler;
pub mod prediect;
mod sc_manager;
mod shared;
pub mod zero_copy;

type HandlerIDAllocator = LeakFromUsizeAllocator<HandlerID, ForwardWrapper>;

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
    futexs: FutexSet,
    id_allocator: HandlerIDAllocator,
    parent: Weak<Predicter>,
    predict: Arc<Predicter>,
}

impl MapSegment {
    pub fn new(page_table: Arc<SyncUnsafeCell<PageTable>>) -> Self {
        Self {
            page_table,
            handlers: HandlerManager::new(),
            sc_manager: SCManager::new(),
            futexs: FutexSet::new(),
            id_allocator: HandlerIDAllocator::default(),
            parent: Weak::new(),
            predict: Arc::new(Predicter::new()),
        }
    }
    pub fn fetch_futex(&mut self, ua: UserAddr<u32>) -> &mut OwnFutex {
        self.futexs.fetch_create(ua, || {
            !self.handlers.get(ua.floor()).unwrap().shared_always()
        })
    }
    pub fn try_fetch_futex(&mut self, ua: UserAddr<u32>) -> Option<&mut OwnFutex> {
        self.futexs.try_fetch(ua)
    }
    /// ?????? range ????????????????????? URange
    pub fn find_free_range(&self, range: URange, n: PageCount) -> Option<URange> {
        self.handlers.find_free_range(range, n)
    }
    /// ?????????????????????????????? ?????? start >= end ????????? Err(())
    pub fn range_is_free(&self, range: URange) -> Result<(), ()> {
        self.handlers.range_is_free(range)
    }
    /// ??????????????????????????? ?????? panic
    ///
    /// ????????????????????? ?????????????????????
    pub fn force_push(
        &mut self,
        r: URange,
        h: Box<dyn UserAreaHandler>,
        allocator: &mut dyn FrameAllocator,
    ) -> SysR<()> {
        debug_assert!(r.start < r.end);
        let pt = pt!(self);
        let h = self.handlers.try_push(r.clone(), h).ok().unwrap();
        let id = self.id_allocator.alloc();
        h.init(id, pt, r.clone(), allocator)
            .inspect_err(|_e| self.unmap(r, allocator))
    }
    fn release_impl<'a>(
        pt: &'a mut PageTable,
        sc_manager: &'a mut SCManager,
        allocator: &'a mut dyn FrameAllocator,
    ) -> impl FnMut(Box<dyn UserAreaHandler>, URange) + 'a {
        move |mut h: Box<dyn UserAreaHandler>, r: URange| {
            stack_trace!();
            let pt = pt as *mut PageTable;
            macro_rules! pt {
                () => {
                    unsafe { &mut *pt }
                };
            }
            let shared_release = |addr| {
                stack_trace!();
                let pte = pt!().try_get_pte_user(addr).unwrap();
                debug_assert!(pte.is_leaf());
                pte.reset();
            };
            let unique_release = |addr| h.unmap_ua(pt!(), addr, allocator);
            // ???????????????
            sc_manager.remove_release(r.clone(), shared_release, unique_release);
            // ?????????????????????????????????????????????????????????????????????????????????
            h.unmap(pt!(), r, allocator);
        }
    }
    /// ???????????????????????????
    pub fn unmap(&mut self, r: URange, allocator: &mut dyn FrameAllocator) {
        debug_assert!(r.start < r.end);
        let sc_manager = &mut self.sc_manager; // stupid borrow checker
        let release = Self::release_impl(pt!(self), sc_manager, allocator);
        self.handlers.remove(r.clone(), release);
        self.futexs.remove(r);
    }
    pub fn clear(&mut self, allocator: &mut dyn FrameAllocator) {
        let sc_manager = &mut self.sc_manager;
        let release = Self::release_impl(pt!(self), sc_manager, allocator);
        self.handlers.clear(release);
        self.futexs.clear();
        assert!(sc_manager.is_empty());
    }
    pub fn replace(
        &mut self,
        r: URange,
        h: Box<dyn UserAreaHandler>,
        allocator: &mut dyn FrameAllocator,
    ) -> SysR<()> {
        debug_assert!(r.start < r.end);
        self.unmap(r.clone(), allocator);
        self.force_push(r, h, allocator)
    }
    pub fn replace_not_release(
        &mut self,
        r: URange,
        h: Box<dyn UserAreaHandler>,
        allocator: &mut dyn FrameAllocator,
    ) -> SysR<()> {
        debug_assert!(r.start < r.end);
        let pt = pt!(self);
        // ??????????????????
        self.handlers.remove(r.clone(), |_, _| ());
        let h = self.handlers.try_push(r.clone(), h).ok().unwrap();
        let id = self.id_allocator.alloc();
        h.init_no_release(id, pt, r.clone(), allocator)
            .inspect_err(|_e| self.unmap(r, allocator))
    }
    /// ???????????? async ????????? panic
    pub fn force_map(&mut self, r: URange, allocator: &mut dyn FrameAllocator) -> SysR<()> {
        stack_trace!();
        debug_assert!(r.start < r.end);
        let pt = pt!(self);
        let h = self.handlers.range_contain_mut(r.clone()).unwrap();
        h.map(pt, r, allocator).map_err(|e| match e {
            TryRunFail::Async(_a) => panic!(),
            TryRunFail::Error(e) => e,
        })
    }
    /// ?????????????????????????????????????????? ??????????????????????????????
    ///
    /// TODO: ?????? copy_map??????????????????????????????
    pub fn force_write_range(
        &mut self,
        r: URange,
        data: &mut dyn FrameDataIter,
        allocator: &mut dyn FrameAllocator,
    ) -> SysR<()> {
        stack_trace!();
        debug_assert!(r.start < r.end);
        self.force_map(r.clone(), allocator)?;
        let pt = pt!(self);
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
        allocator: &mut dyn FrameAllocator,
    ) -> TryR<DynDropRun<(UserAddr4K, Asid)>, Box<dyn AsyncHandler>> {
        debug_assert!(access.user);
        // println!("page_fault {:#x}", addr.into_usize());
        let h = self
            .handlers
            .get_mut(addr)
            .ok_or(TryRunFail::Error(SysError::EFAULT))?;

        let pt = pt!(self);
        let pte = match pt.try_get_pte_user(addr) {
            // ???????????????????????????, ?????????????????????
            None => {
                let perm = h.perm();
                access.check(perm).map_err(|()| SysError::EFAULT)?;
                if let Some(page) = h.try_rd_only_shared(addr, allocator) {
                    let pte = pt.get_pte_user(addr, allocator)?;
                    if access.write {
                        let x = match zero_copy::request_and_take_own(&page) {
                            Some(x) => x,
                            None => {
                                let x = allocator.alloc()?;
                                let dst = x.data().as_usize_array_mut();
                                faster::page_copy(dst, page.as_usize_array());
                                x
                            }
                        };
                        pte.alloc_by_frame(perm, x.consume());
                    } else {
                        let (sc, pa) = page.into_inner();
                        self.sc_manager.insert_by(addr, sc);
                        pte.alloc_by_frame(perm, pa);
                        pte.become_shared(false);
                    }
                    return Ok(pt.flush_va_asid_fn(addr));
                }
                return h.page_fault(pt, addr, access, allocator);
            }
            Some(a) => a,
        };
        // ??????pte??????X?????????, ???????????????????????????, ????????????
        if access.exec {
            debug_assert!(!h.executable());
            debug_assert!(!pte.executable());
            return Err(TryRunFail::Error(SysError::EFAULT));
        }
        debug_assert!(access.write);
        if !h.unique_writable() {
            // ???pte??????????????????
            return Err(TryRunFail::Error(SysError::EFAULT));
        }
        // ????????????????????????????????????
        stack_trace!();
        // COW??????
        debug_assert!(pte.shared());
        self.predict.insert(addr);

        if let Some(predictor) = self.parent.upgrade() {
            predictor.insert(addr);
        }
        // ???????????????1????????????????????????
        if self.sc_manager.try_remove_unique(addr) {
            if PRINT_PAGE_FAULT {
                println!("this shared page is unique");
            }
            pte.clear_shared();
            pte.set_writable();
            return Ok(pt!(self).flush_va_asid_fn(addr));
        }
        if PRINT_PAGE_FAULT {
            println!("copy to new page");
        }
        // ?????????????????????, ????????????????????????
        let x = allocator.alloc()?;
        faster::page_copy(
            x.data().as_usize_array_mut(),
            pte.phy_addr().into_ref().as_usize_array(),
        );
        // ??????????????????????????????, ?????????????????????????????????????????????????????????????????????, ????????????
        if self.sc_manager.remove_ua(addr) {
            if PRINT_PAGE_FAULT {
                println!("release old shared page");
            }
            unsafe { pte.dealloc_by(allocator) };
        }
        // ????????????
        *pte = PageTableEntry::new(x.consume().into(), h.map_perm());
        Ok(pt!(self).flush_va_asid_fn(addr))
    }
    /// ???????????????????????????????????????, ??????????????????, ????????????????????????????????????
    ///
    /// ????????? / ???????????????: ????????????????????????????????????
    ///
    /// COW ?????????: ??????????????? ?????????????????????
    pub fn modify_perm(&mut self, r: URange, perm: PTEFlags) -> SysR<()> {
        stack_trace!();
        debug_assert!(r.start < r.end);
        // 1. ???????????????max?????????
        // 2. ????????????
        // 3. ?????????????????????
        let (sr, sh) = self.handlers.get_rv(r.start).ok_or(SysError::EFAULT)?;
        sh.new_perm_check(perm).map_err(|_| SysError::EACCES)?;
        // ????????????????????????
        let mut cur_end = sr.end;
        for (r, h) in self.handlers.range(r.clone()) {
            if r.start == sr.start {
                continue;
            }
            if r.start != cur_end {
                return Err(SysError::EFAULT);
            }
            h.new_perm_check(perm).map_err(|_| SysError::EACCES)?;
            cur_end = r.end;
        }
        if cur_end < r.end {
            return Err(SysError::EFAULT);
        }
        // ????????????
        self.handlers.split_at_maybe(r.start);
        self.handlers.split_at_maybe(r.end);
        // ???????????????????????????????????????
        let pt = pt!(self);
        for (xr, h) in self.handlers.range_mut(r) {
            h.modify_perm(perm);
            if h.shared_always() {
                for (_addr, pte) in pt.valid_pte_iter(xr) {
                    pte.set_rwx(perm);
                }
            } else {
                for (_addr, pte) in pt.valid_pte_iter(xr) {
                    if !pte.shared() {
                        pte.set_rwx(perm);
                    }
                }
            }
        }
        Ok(())
    }
    /// ???????????? fork
    ///
    /// ??????????????????????????????????????????, ???????????????????????????
    ///
    /// ???????????????????????? may_shared()
    pub fn fork(&mut self) -> SysR<Self> {
        stack_trace!();
        let src = pt!(self);
        let mut dst = PageTable::from_global(asid::alloc_asid())?;
        let allocator = &mut frame::default_allocator();
        let mut new_sm = SCManager::new();
        // flush ??????????????????
        let flush = src.flush_asid_fn();
        let mut err_1 = Ok(());

        let mut predict = self.predict.take_in_order().into_iter().peekable();

        for (r, h) in self.handlers.iter_mut() {
            stack_trace!();
            match h.may_shared() {
                Some(shared_writable) => {
                    // ?????????????????????
                    let mut err_2 = Ok(());
                    for (addr, src) in src.valid_pte_iter(r.clone()) {
                        // ???????????????????????????PTE
                        let dst = match dst.get_pte_user(addr, allocator) {
                            Ok(x) => x,
                            Err(e) => {
                                err_1 = Err((r.clone(), e.into()));
                                err_2 = Err(addr);
                                break;
                            }
                        };

                        let mut cow_hit = false;
                        while let Some(&ua) = predict.peek() {
                            if ua > addr {
                                break;
                            }
                            predict.next();
                            if ua < addr {
                                continue;
                            }
                            cow_hit = true;
                            break;
                        }
                        if cow_hit && !shared_writable && h.unique_writable() {
                            // ?????????COW????????????????????????????????????, ????????????
                            match dst.alloc_by(h.perm(), allocator) {
                                Ok(()) => (),
                                Err(e) => {
                                    err_1 = Err((r.clone(), e.into()));
                                    err_2 = Err(addr);
                                    break;
                                }
                            }
                            faster::page_copy(
                                dst.phy_addr().into_ref().as_usize_array_mut(),
                                src.phy_addr().into_ref().as_usize_array(),
                            );
                        } else {
                            stack_trace!();
                            debug_assert!(!dst.is_valid(), "fork addr: {:#x}", addr.into_usize());
                            // ???????????????
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
                    }
                    // roll back inner
                    let e_addr = match err_2 {
                        Ok(()) => continue,
                        Err(x) => x,
                    };
                    stack_trace!();
                    // error happen
                    // todo ?????????????????????
                    for (addr, dst) in dst.valid_pte_iter(r) {
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
                None => match h.copy_map(src, &mut dst, r.clone(), allocator) {
                    Ok(()) => (),
                    Err(e) => {
                        err_1 = Err((r, e));
                        break;
                    }
                },
            }
        }

        stack_trace!();
        if err_1.is_ok() {
            let new_ms = MapSegment {
                page_table: Arc::new(SyncUnsafeCell::new(dst)),
                handlers: self.handlers.fork(),
                sc_manager: new_sm,
                futexs: self.futexs.fork(),
                id_allocator: self.id_allocator.clone(),
                parent: Arc::downgrade(&self.predict),
                predict: Arc::new(Predicter::new()),
            };
            stack_trace!();
            return Ok(new_ms);
        }
        stack_trace!();
        // ????????????
        let (rr, e) = err_1.unwrap_err();
        new_sm.check_remove_all();
        for (r, h) in self.handlers.iter_mut() {
            if r == rr {
                break;
            }
            match h.may_shared() {
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
                    h.unmap(&mut dst, r, allocator);
                }
            }
        }
        flush.run();
        Err(e)
    }

    pub fn clear_except_program(&mut self, allocator: &mut dyn FrameAllocator) {
        let sc_manager = &mut self.sc_manager;
        let release = Self::release_impl(pt!(self), sc_manager, allocator);
        self.handlers.clear_except_program(release);
        self.futexs.clear();
    }
}
