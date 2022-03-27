use alloc::{boxed::Box, sync::Arc};

use crate::{
    memory::{
        address::UserAddr4K,
        allocator::frame,
        asid::Asid,
        page_table::PTEFlags,
        user_space::{AccessType, UserArea},
        PageTable, UserSpace,
    },
    process::{Dead, Process},
    syscall::SysError,
    tools::{
        self,
        range::URange,
        xasync::{AsyncR, HandlerID, TryR},
    },
};

pub mod delay;
pub mod manager;
pub mod map_all;

pub trait UserAreaHandler: Send + 'static {
    fn id(&self) -> HandlerID;
    fn perm(&self) -> PTEFlags;
    fn map_perm(&self) -> PTEFlags {
        self.perm() | PTEFlags::U | PTEFlags::V
    }
    fn user_area(&self, range: URange) -> UserArea {
        UserArea::new(range, self.perm())
    }
    /// 唯一存在时的写标志
    fn unique_writable(&self) -> bool {
        self.perm().contains(PTEFlags::W)
    }
    /// 共享分配写标志 当 unique_writable 为 false 时不可返回 true
    ///
    /// Some(x): 可共享，x为共享后的写标志位 由页面管理器代理共享与释放
    ///
    /// None: 不可共享，使用 copy_map 复制
    fn shared_writable(&self) -> Option<bool> {
        Some(false)
        // None
    }
    fn executable(&self) -> bool {
        self.perm().contains(PTEFlags::X)
    }
    /// 新加入管理器时将调用此函数 保证范围内无映射 此函数是唯一标记 &mut 的函数
    ///
    /// 必须设置正确的id
    fn init(&mut self, id: HandlerID, pt: &mut PageTable, all: URange) -> Result<(), SysError>;
    /// map range范围内的全部地址，必须跳过已经分配的区域
    ///
    /// try_xx user_space获得页表所有权，进程一定是有效的
    ///
    /// 如果操作失败且返回Async则改为调用 a_map.
    fn map(&self, pt: &mut PageTable, range: URange) -> TryR<(), Box<dyn AsyncHandler>>;
    /// 从 src 复制 range 到 dst, dst 获得所有权
    ///
    /// 保证范围内无有效映射
    fn copy_map(&self, src: &mut PageTable, dst: &mut PageTable, r: URange)
        -> Result<(), SysError>;
    /// 如果操作失败且返回Async则改为调用 a_page_fault.
    fn page_fault(
        &self,
        pt: &mut PageTable,
        addr: UserAddr4K,
        access: AccessType,
    ) -> TryR<(), Box<dyn AsyncHandler>>;
    /// 所有权取消映射
    ///
    /// 不保证范围内全部映射
    ///
    /// 保证范围内不存在共享映射
    ///
    /// 调用后页表必须移除映射
    fn unmap(&self, pt: &mut PageTable, range: URange);
    /// 所有权取消映射一个页
    ///
    /// 保证此地址被映射 保证不是共享映射
    ///
    /// 调用后页表必须移除映射
    fn unmap_ua(&self, pt: &mut PageTable, addr: UserAddr4K);
    /// 以 addr 为界切除 all 左侧, 即返回 all.start..addr, 自身变为 addr..all.end
    ///
    /// 某些 handler 可能使用偏移量定位, 这时必须重写此函数 返回值使用相同的 id
    fn split_l(&mut self, _addr: UserAddr4K, _all: URange) -> Box<dyn UserAreaHandler> {
        self.box_clone()
    }
    /// 以 addr 为界切除 all 右侧, 即返回 addr..all.end, 自身变为 all.start..addr
    ///
    /// 某些 handler 可能使用偏移量定位, 这时必须重写此函数 返回值使用相同的 id
    fn split_r(&mut self, _addr: UserAddr4K, _all: URange) -> Box<dyn UserAreaHandler> {
        self.box_clone()
    }
    /// 复制
    fn box_clone(&self) -> Box<dyn UserAreaHandler>;
    /// 进行映射, 跳过已经分配空间的区域
    fn default_map(&self, pt: &mut PageTable, range: URange) -> TryR<(), Box<dyn AsyncHandler>> {
        stack_trace!();
        if range.start >= range.end {
            return Ok(());
        }
        let perm = self.perm();
        let allocator = &mut frame::defualt_allocator();
        for r in pt.each_pte_iter(range) {
            let (_addr, pte) = r?;
            if pte.is_valid() {
                continue;
            }
            pte.alloc_by(perm, allocator)?;
        }
        Ok(())
    }
    /// 利用全局内存分配器分配内存，复制src中存在的页
    fn default_copy_map(
        &self,
        src: &mut PageTable,
        dst: &mut PageTable,
        r: URange,
    ) -> Result<(), SysError> {
        let allocator = &mut frame::defualt_allocator();
        for a in tools::range::ur_iter(r) {
            let src = src.try_get_pte_user(a);
            if src.is_none() {
                continue;
            }
            let src = src.unwrap().phy_addr().into_ref().as_usize_array();
            let dst = dst.get_pte_user(a, allocator)?;
            debug_assert!(!dst.is_valid());
            dst.alloc_by(self.map_perm(), allocator)?;
            dst.phy_addr()
                .into_ref()
                .as_usize_array_mut()
                .copy_from_slice(src);
        }
        Ok(())
    }
    /// 所有权释放页表中存在映射的空间
    fn default_unmap(&self, pt: &mut PageTable, range: URange) {
        stack_trace!();
        pt.unmap_user_range_lazy(self.user_area(range), &mut frame::defualt_allocator());
    }
    /// 所有权释放页表中存在映射的空间
    fn default_unmap_ua(&self, pt: &mut PageTable, addr: UserAddr4K) {
        stack_trace!();
        let pte = pt.try_get_pte_user(addr).unwrap();
        debug_assert!(pte.is_leaf());
        unsafe { pte.dealloc_by(&mut frame::defualt_allocator()) };
    }
}

pub trait AsyncHandler: Send + 'static {
    fn id(&self) -> HandlerID;
    fn a_map(self: Box<Self>, sh: SpaceHolder, range: URange) -> AsyncR<Asid>;
    fn a_page_fault(
        self: Box<Self>,
        sh: SpaceHolder,
        addr: UserAddr4K,
    ) -> AsyncR<(UserAddr4K, Asid)>;
}

/// 数据获取完毕后才获取锁获取锁
pub struct SpaceHolder(Arc<Process>);

impl SpaceHolder {
    pub fn new(p: Arc<Process>) -> Self {
        Self(p)
    }
    fn space_run<T, F: FnOnce(&mut UserSpace) -> T>(&self, f: F) -> Result<T, Dead> {
        self.0.alive_then(|a| f(&mut a.user_space))
    }
    fn page_table_run<T, F: FnOnce(&mut PageTable) -> T>(&self, f: F) -> Result<T, Dead> {
        self.0.alive_then(|a| f(a.user_space.page_table_mut()))
    }
}
