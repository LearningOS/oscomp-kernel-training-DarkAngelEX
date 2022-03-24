use alloc::boxed::Box;

use crate::{
    memory::{allocator::frame, asid, page_table::PageTableEntry},
    syscall::SysError,
    tools::{
        self,
        allocator::from_usize_allocator::LeakFromUsizeAllocator,
        range::URange,
        xasync::{HandlerID, TryRunFail},
        ForwardWrapper,
    },
};

use self::{
    handler::{manager::HandlerManager, UserAreaHandler},
    sc_manager::SCManager,
};

use super::{allocator::frame::iter::FrameDataIter, PageTable};

pub mod handler;
mod sc_manager;

type HandlerAllocator = LeakFromUsizeAllocator<HandlerID, ForwardWrapper>;

/// own by user_space
pub struct MapSegment {
    handlers: HandlerManager,
    sc_manager: SCManager,
    id_allocator: HandlerAllocator,
}

impl MapSegment {
    pub const fn new() -> Self {
        Self {
            handlers: HandlerManager::new(),
            sc_manager: SCManager::new(),
            id_allocator: HandlerAllocator::default(),
        }
    }
    /// 范围必须不存在映射 否则 panic
    ///
    /// 返回初始化结果 失败则撤销映射
    pub fn force_push(
        &mut self,
        pt: &mut PageTable,
        r: URange,
        h: Box<dyn UserAreaHandler>,
    ) -> Result<(), SysError> {
        let h = self.handlers.try_push(r.clone(), h).ok().unwrap();
        let id = self.id_allocator.alloc();
        h.init(id, pt, r.clone())
            .inspect_err(|_e| self.unmap(pt, r))
    }
    /// 释放存在映射的空间
    pub fn unmap(&mut self, pt: &mut PageTable, r: URange) {
        let sc_manager = &mut self.sc_manager; // stupid borrow checker
        self.handlers.remove_range(r, |h, r| {
            sc_manager.remove_release(r.clone(), |addr| h.unmap_ua(pt, addr));
            h.unmap(pt, r);
        })
    }
    pub fn clear(&mut self, pt: &mut PageTable) {
        let sc_manager = &mut self.sc_manager;
        self.handlers.clear(|h, r| {
            sc_manager.remove_release(r.clone(), |addr| h.unmap_ua(pt, addr));
            h.unmap(pt, r);
        });
        assert!(sc_manager.is_empty());
    }
    pub fn replace(
        &mut self,
        pt: &mut PageTable,
        r: URange,
        h: Box<dyn UserAreaHandler>,
    ) -> Result<(), SysError> {
        self.unmap(pt, r.clone());
        self.force_push(pt, r, h)
    }
    /// 如果进入 async 状态将 panic
    pub fn force_map(&self, pt: &mut PageTable, r: URange) -> Result<(), SysError> {
        let h = self.handlers.range_contain(r.clone()).unwrap();
        h.map(pt, r).map_err(|e| match e {
            TryRunFail::Async(_a) => panic!(),
            TryRunFail::Error(e) => e,
        })
    }
    /// 此函数可以向只读映射写入数据 但不能修改只读共享页
    pub fn force_write_range(
        &self,
        pt: &mut PageTable,
        r: URange,
        mut data: impl FrameDataIter,
    ) -> Result<(), SysError> {
        self.force_map(pt, r.clone())?;
        for addr in tools::range::ur_iter(r) {
            pt.force_convert_user(addr, |pte| {
                assert!(!pte.shared() || pte.writable());
                let _ = data.write_to(pte.phy_addr().into_ref().as_bytes_array_mut());
            });
        }
        Ok(())
    }
    /// 共享优化 fork
    ///
    /// 发生错误时回退到执行前的状态
    pub fn fork(&mut self, src: &mut PageTable) -> Result<(Self, PageTable), SysError> {
        let mut dst = PageTable::from_global(asid::alloc_asid())?;
        let allocator = &mut frame::defualt_allocator();
        let mut sm = SCManager::new();

        let mut error = Ok(());
        for (r, h) in self.handlers.iter() {
            match h.shared_writable() {
                Some(shared_writable) => {
                    for (addr, src) in src.valid_pte_iter(r.clone()) {
                        let dst = match dst.get_pte_user(addr, allocator) {
                            Ok(x) => x,
                            Err(e) => {
                                error = Err((r, e.into()));
                                break;
                            }
                        };
                        let sc = if !src.shared() {
                            src.become_shared(shared_writable);
                            self.sc_manager.insert(addr)
                        } else {
                            self.sc_manager.clone_ua(addr)
                        };
                        sm.insert_by(addr, sc);
                        *dst = *src;
                    }
                }
                None => match h.copy_map(src, &mut dst, r.clone()) {
                    Ok(()) => (),
                    Err(e) => {
                        error = Err((r, e));
                        break;
                    }
                },
            }
        }
        // 错误回退
        if let Err((rr, e)) = error {
            sm.check_remove_all();
            for (r, h) in self.handlers.iter() {
                if r == rr {
                    break;
                }
                match h.shared_writable() {
                    Some(_) => {
                        for addr in tools::range::ur_iter(r.clone()) {
                            if let Some(pte) = dst.try_get_pte_user(addr) {
                                assert!(pte.shared());
                                *pte = PageTableEntry::empty();
                                if self.sc_manager.try_remove_unique(addr) {
                                    let pte = src.try_get_pte_user(addr).unwrap();
                                    pte.become_unique(h.unique_writable());
                                }
                            }
                        }
                    }
                    None => {
                        h.unmap(&mut dst, r);
                    }
                }
            }
            return Err(e);
        }
        let new_ms = MapSegment {
            handlers: self.handlers.fork(),
            sc_manager: sm,
            id_allocator: self.id_allocator.clone(),
        };
        Ok((new_ms, dst))
    }
}
