use alloc::{boxed::Box, sync::Arc};

use crate::{
    fs::File,
    memory::{
        address::UserAddr4K,
        allocator::frame,
        asid::Asid,
        page_table::{PTEFlags, PageTableEntry},
        user_space::{AccessType, UserArea},
        PageTable,
    },
    process::Process,
    syscall::SysError,
    tools::{
        self,
        allocator::TrackerAllocator,
        range::URange,
        xasync::{AsyncR, HandlerID, TryR, TryRunFail},
    },
};

pub mod delay;
pub mod manager;
pub mod map_all;
pub mod mmap;

pub trait UserAreaHandler: Send + 'static {
    fn id(&self) -> HandlerID;
    fn perm(&self) -> PTEFlags;
    fn map_perm(&self) -> PTEFlags {
        self.perm() | PTEFlags::U | PTEFlags::D | PTEFlags::A | PTEFlags::V
    }
    fn user_area(&self, range: URange) -> UserArea {
        UserArea::new(range, self.perm())
    }
    /// 唯一存在时的写标志
    fn unique_writable(&self) -> bool {
        self.perm().contains(PTEFlags::W)
    }
    fn using_cow(&self) -> bool {
        true
    }
    fn shared_always(&self) -> bool {
        false
    }
    /// return shared_writable
    fn may_shared(&self) -> Option<bool> {
        if self.shared_always() {
            Some(self.unique_writable())
        } else if self.using_cow() {
            Some(false)
        } else {
            None
        }
    }
    fn executable(&self) -> bool {
        self.perm().contains(PTEFlags::X)
    }
    /// 新加入管理器时将调用此函数 保证范围内无映射 此函数是唯一标记 &mut 的函数
    ///
    /// 必须设置正确的id
    fn init(&mut self, id: HandlerID, pt: &mut PageTable, all: URange) -> Result<(), SysError>;
    /// 此项初始化后禁止修改
    fn max_perm(&self) -> PTEFlags {
        PTEFlags::R | PTEFlags::W | PTEFlags::X | PTEFlags::U
    }
    /// 此函数无需重写
    fn new_perm_check(&self, perm: PTEFlags) -> Result<(), ()> {
        tools::bool_result(perm & self.map_perm() == perm)
    }
    /// 修改整个段的perm 段管理器在调用此函数之前会调用 new_perm_check 进行检查
    ///
    /// 不能有未映射的页面
    fn modify_perm(&mut self, perm: PTEFlags);
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
    fn default_page_fault(
        &self,
        pt: &mut PageTable,
        addr: UserAddr4K,
        access: AccessType,
    ) -> TryR<(), Box<dyn AsyncHandler>> {
        stack_trace!();
        access
            .check(self.perm())
            .map_err(|_| TryRunFail::Error(SysError::EFAULT))?;
        // 可能同时进入的另一个线程已经处理了这个页错误
        pt.force_map_user(
            addr,
            || {
                let pa = frame::defualt_allocator().alloc()?.consume();
                pa.as_bytes_array_mut().fill(0);
                Ok(PageTableEntry::new(pa.into(), self.map_perm()))
            },
            &mut frame::defualt_allocator(),
        )?;
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

pub trait AsyncHandler: Send + Sync {
    fn id(&self) -> HandlerID;
    fn perm(&self) -> PTEFlags;
    fn a_map<'a>(&'a self, process: &'a Process, range: URange) -> AsyncR<'a, Asid>;
    fn a_page_fault<'a>(&'a self, process: &'a Process, addr: UserAddr4K) -> AsyncR<'a, Asid>;
}

pub struct FileAsyncHandler {
    id: HandlerID,
    perm: PTEFlags,
    start: UserAddr4K,
    offset: usize,
    file: Arc<dyn File>,
}
impl FileAsyncHandler {
    pub fn new(
        id: HandlerID,
        perm: PTEFlags,
        start: UserAddr4K,
        offset: usize,
        file: Arc<dyn File>,
    ) -> Self {
        Self {
            id,
            perm,
            start,
            offset,
            file,
        }
    }
}

impl AsyncHandler for FileAsyncHandler {
    fn id(&self) -> HandlerID {
        self.id
    }
    fn perm(&self) -> PTEFlags {
        self.perm | PTEFlags::U | PTEFlags::D | PTEFlags::A | PTEFlags::V
    }
    fn a_map<'a>(&'a self, process: &'a Process, range: URange) -> AsyncR<Asid> {
        Box::pin(async move {
            stack_trace!();
            if !self.file.can_read_offset() {
                return Err(SysError::EACCES);
            }
            let mut asid = None;
            let allocator = &mut frame::defualt_allocator();
            for addr in tools::range::ur_iter(range) {
                debug_assert!(addr >= self.start);
                let offset = addr.into_usize() - self.start.into_usize() + self.offset;
                let frame = allocator.alloc()?;
                let n = self
                    .file
                    .read_at_kernel(offset, frame.data().as_bytes_array_mut())
                    .await?;
                frame.data().as_bytes_array_mut()[n..].fill(0);
                asid = Some(process.alive_then(|a| -> Result<Asid, SysError> {
                    let pte = a
                        .user_space
                        .page_table_mut()
                        .get_pte_user(addr, allocator)?;
                    if !pte.is_valid() {
                        pte.alloc_by_frame(self.perm(), frame.consume());
                    }
                    Ok(a.asid())
                })??);
            }
            let asid = match asid {
                None => process.alive_then(|a| a.asid())?,
                Some(asid) => asid,
            };
            Ok(asid)
        })
    }
    fn a_page_fault<'a>(&'a self, process: &'a Process, addr: UserAddr4K) -> AsyncR<Asid> {
        Box::pin(async move {
            stack_trace!();
            if !self.file.can_read_offset() {
                return Err(SysError::EACCES);
            }
            let allocator = &mut frame::defualt_allocator();
            debug_assert!(addr >= self.start);
            let offset = addr.into_usize() - self.start.into_usize() + self.offset;
            let frame = allocator.alloc()?;
            let n = self
                .file
                .read_at_kernel(offset, frame.data().as_bytes_array_mut())
                .await?;
            frame.data().as_bytes_array_mut()[n..].fill(0);
            let asid = process.alive_then(|a| -> Result<Asid, SysError> {
                let pte = a
                    .user_space
                    .page_table_mut()
                    .get_pte_user(addr, allocator)?;
                if !pte.is_valid() {
                    pte.alloc_by_frame(self.perm(), frame.consume());
                }
                Ok(a.asid())
            })??;
            Ok(asid)
        })
    }
}
