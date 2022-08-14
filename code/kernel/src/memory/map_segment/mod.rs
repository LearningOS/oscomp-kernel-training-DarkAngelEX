use alloc::{
    boxed::Box,
    sync::{Arc, Weak},
};
use ftl_util::error::SysR;

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
    /// 查找 range 内第一个空闲的 URange
    pub fn find_free_range(&self, range: URange, n: PageCount) -> Option<URange> {
        self.handlers.find_free_range(range, n)
    }
    /// 检查区间是否是空闲的 如果 start >= end 将返回 Err(())
    pub fn range_is_free(&self, range: URange) -> Result<(), ()> {
        self.handlers.range_is_free(range)
    }
    /// 范围必须不存在映射 否则 panic
    ///
    /// 返回初始化结果 失败则撤销映射
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
            // 释放共享页
            sc_manager.remove_release(r.clone(), shared_release, unique_release);
            // 共享页管理器只包括共享页，因此还要释放本进程分配的页面
            h.unmap(pt!(), r, allocator);
        }
    }
    /// 释放存在映射的空间
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
        // 不释放旧内存
        self.handlers.remove(r.clone(), |_, _| ());
        let h = self.handlers.try_push(r.clone(), h).ok().unwrap();
        let id = self.id_allocator.alloc();
        h.init_no_release(id, pt, r.clone(), allocator)
            .inspect_err(|_e| self.unmap(r, allocator))
    }
    /// 如果进入 async 状态将 panic
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
    /// 此函数可以向只读映射写入数据 但不能修改只读共享页
    ///
    /// TODO: 使用 copy_map获取只读共享页所有权
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
            // 这个页还没有被映射, 映射一个唯一页
            None => return h.page_fault(pt, addr, access, allocator),
            Some(a) => a,
        };
        // 如果pte没有X标志位, 那一定是用户故意的, 操作失败
        if access.exec {
            debug_assert!(!h.executable());
            debug_assert!(!pte.executable());
            return Err(TryRunFail::Error(SysError::EFAULT));
        }
        debug_assert!(access.write);
        if !h.unique_writable() {
            // 此pte禁止写入操作
            return Err(TryRunFail::Error(SysError::EFAULT));
        }
        // 尝试获取一个写权限的页面
        stack_trace!();
        // COW操作
        debug_assert!(pte.shared());
        self.predict.insert(addr);
        if let Some(predictor) = self.parent.upgrade() {
            predictor.insert(addr);
        }
        // 引用计数为1时直接修改写权限
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
        // 分配一个新的页, 从原页面复制数据
        let x = allocator.alloc()?;
        x.ptr()
            .as_usize_array_mut()
            .copy_from_slice(pte.phy_addr().into_ref().as_usize_array());
        // 递减旧的页的引用计数, 如果它是最后一个说明在这期间有其他进程释放了它, 将他释放
        if self.sc_manager.remove_ua(addr) {
            if PRINT_PAGE_FAULT {
                println!("release old shared page");
            }
            unsafe { pte.dealloc_by(allocator) };
        }
        // 设置页面
        *pte = PageTableEntry::new(x.consume().into(), h.map_perm());
        Ok(pt!(self).flush_va_asid_fn(addr))
    }
    /// 必须区间内全部内存页都存在, 否则操作失败, 操作结束后手动在锁外刷表
    ///
    /// 唯一页 / 永久共享页: 修改页表标志位和段标志位
    ///
    /// COW 共享页: 不修改页表 只修改段标志位
    pub fn modify_perm(&mut self, r: URange, perm: PTEFlags) -> SysR<()> {
        stack_trace!();
        debug_assert!(r.start < r.end);
        // 1. 检查区间与max标志位
        // 2. 边缘切割
        // 3. 修改段内标志位
        let (sr, sh) = self.handlers.get_rv(r.start).ok_or(SysError::EFAULT)?;
        sh.new_perm_check(perm).map_err(|_| SysError::EACCES)?;
        // 检测段是否都存在
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
        // 边缘切割
        self.handlers.split_at_maybe(r.start);
        self.handlers.split_at_maybe(r.end);
        // 保证遍历到的都不会跨越边界
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
    /// 共享优化 fork
    ///
    /// 发生错误时回退到执行前的状态, 不会让操作系统崩掉
    ///
    /// 将写标志位设置为 may_shared()
    pub fn fork(&mut self) -> SysR<Self> {
        stack_trace!();
        let src = pt!(self);
        let mut dst = PageTable::from_global(asid::alloc_asid())?;
        let allocator = &mut frame::default_allocator();
        let mut new_sm = SCManager::new();
        // flush 析构时将刷表
        let flush = src.flush_asid_fn();
        let mut err_1 = Ok(());

        let mut predict = self.predict.take_in_order().into_iter().peekable();

        for (r, h) in self.handlers.iter_mut() {
            stack_trace!();
            match h.may_shared() {
                Some(shared_writable) => {
                    // 用来错误回退段
                    let mut err_2 = Ok(());
                    for (addr, src) in src.valid_pte_iter(r.clone()) {
                        // 在新页表中生成一个PTE
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
                            // 这里是COW预测器预测的会缺页的位置, 提前映射
                            match dst.alloc_by(h.perm(), allocator) {
                                Ok(()) => (),
                                Err(e) => {
                                    err_1 = Err((r.clone(), e.into()));
                                    err_2 = Err(addr);
                                    break;
                                }
                            }
                            dst.phy_addr()
                                .into_ref()
                                .as_usize_array_mut()
                                .copy_from_slice(src.phy_addr().into_ref().as_usize_array());
                        } else {
                            stack_trace!();
                            debug_assert!(!dst.is_valid(), "fork addr: {:#x}", addr.into_usize());
                            // 变成共享页
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
                    // todo 没有处理预测页
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
        // 错误回退
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
