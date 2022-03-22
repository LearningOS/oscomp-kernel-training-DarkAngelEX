pub mod alloc_handler;
pub mod manager;

use core::ops::Range;

use alloc::{boxed::Box, sync::Arc};

use crate::{
    memory::{
        address::{PageCount, UserAddr, UserAddr4K},
        allocator::frame,
        page_table::PTEFlags,
        user_space::UserArea,
        PageTable,
    },
    process::{Dead, Process},
    tools::{
        self,
        xasync::{AsyncR, HandlerID, TryR},
    },
};

use super::UserSpace;

pub trait UserAreaHandler: Send + 'static {
    fn id(&self) -> HandlerID;
    fn range(&self) -> Range<UserAddr4K>;
    fn contains(&self, value: UserAddr4K) -> bool {
        self.range().contains(&value)
    }
    fn perm(&self) -> PTEFlags;
    fn user_area(&self) -> UserArea {
        UserArea {
            range: self.range(),
            perm: self.perm(),
        }
    }
    fn writable(&self) -> bool {
        self.perm().contains(PTEFlags::W)
    }
    fn execable(&self) -> bool {
        self.perm().contains(PTEFlags::X)
    }
    /// try_xx user_space获得页表所有权，进程一定是有效的.
    ///
    /// 如果操作失败且返回Async则改为调用 a_map.
    fn try_map(
        &self,
        pt: &mut PageTable,
        range: Range<UserAddr4K>,
    ) -> TryR<(), Box<dyn AsyncHandler>>;
    /// 如果操作失败且返回Async则改为调用 a_page_fault.
    ///
    /// 不使用 UserAddr4K 是因为这可能携带信息.
    fn try_page_fault(&self, pt: &mut PageTable, addr: UserAddr)
        -> TryR<(), Box<dyn AsyncHandler>>;
    fn unmap(&self, pt: &mut PageTable, range: Range<UserAddr4K>) -> PageCount;
    fn unmap_all(&self, pt: &mut PageTable) -> PageCount {
        self.unmap(pt, self.range())
    }
    fn default_unmap(&self, pt: &mut PageTable, range: Range<UserAddr4K>) -> PageCount {
        stack_trace!();
        let range = tools::range_limit(range, self.range());
        if range.start >= range.end {
            return PageCount(0);
        }
        pt.unmap_user_range_lazy(
            &UserArea {
                range,
                perm: self.perm(),
            },
            &mut frame::defualt_allocator(),
        )
    }
    fn split_l(&mut self, addr: UserAddr4K) -> Box<dyn UserAreaHandler>;
    fn split_r(&mut self, addr: UserAddr4K) -> Box<dyn UserAreaHandler>;
    fn release(self: Box<Self>, pt: &mut PageTable) -> PageCount {
        pt.unmap_user_range_lazy(&self.user_area(), &mut frame::defualt_allocator())
    }
}

pub trait AsyncHandler {
    fn a_map(self: Arc<Self>, pt: SpaceHolder, range: Range<UserAddr4K>) -> AsyncR<()>;
    fn a_page_fault(self: Arc<Self>, pt: SpaceHolder, addr: UserAddr) -> AsyncR<()>;
}

/// 用来延迟获取锁
pub struct SpaceHolder(Arc<Process>);

impl SpaceHolder {
    pub fn new(p: Arc<Process>) -> Self {
        Self(p)
    }
    fn space_run<T, F: FnOnce(&mut UserSpace) -> T>(&self, f: F) -> Result<T, Dead> {
        self.0.alive_then(|a| f(&mut a.user_space))
    }
    fn page_table_run<T, F: FnOnce(&mut PageTable) -> T>(&self, f: F) -> Result<T, Dead> {
        self.0.alive_then(|a| f(&mut a.user_space.page_table_mut()))
    }
}
