use core::arch::global_asm;

use ftl_util::error::SysR;
use riscv::register::{
    scause::{self, Exception, Scause},
    sepc, stval, stvec,
    utvec::TrapMode,
};

use crate::{
    local,
    memory::{
        address::UserAddr,
        allocator::frame::FrameAllocator,
        user_ptr::{UserReadPtr, UserWritePtr},
        AccessType,
    },
    process::Process,
    syscall::SysError,
    tools::xasync::TryRunFail,
    trap,
    xdebug::PRINT_PAGE_FAULT,
};

use super::UserAccessStatus;

global_asm!(include_str!("check_impl.S"));

const PRINT_CHECK_ERR: bool = false;

pub(super) struct UserCheckImpl<'a>(&'a Process);

impl Drop for UserCheckImpl<'_> {
    fn drop(&mut self) {
        unsafe { trap::set_kernel_default_trap() };
        assert!(Self::status().is_access());
    }
}

/// 通过异常来测试地址权限, 操作系统需要保证内核态不会发生异常, 但允许中断
impl<'a> UserCheckImpl<'a> {
    #[inline(always)]
    pub fn new(process: &'a Process) -> Self {
        assert!(Self::status().is_access());
        unsafe { set_error_handle() };
        Self(process)
    }
    #[inline(always)]
    fn status() -> &'static mut UserAccessStatus {
        &mut local::always_local().user_access_status
    }
    pub fn atomic_u32_check_rough(
        &self,
        ptr: UserReadPtr<u32>,
        allocator: &mut dyn FrameAllocator,
    ) -> SysR<()> {
        let ptr = ptr.as_usize();
        match try_write_user_u32_atomic(ptr) {
            Ok(_) => return Ok(()),
            Err(e) => {
                if PRINT_CHECK_ERR {
                    println!("atomic_u32_check fail!(0) ptr: {:#x} cause: {}", ptr, e)
                }
            }
        }
        self.handle_write_fault_rough(ptr, allocator)
            .inspect_err(|e| {
                if PRINT_CHECK_ERR {
                    println!("atomic_u32_check fail!(1) ptr: {:#x} cause: {}", ptr, e)
                }
            })?;
        try_write_user_u32_atomic(ptr).inspect_err(|e| {
            if PRINT_CHECK_ERR {
                println!("atomic_u32_check fail!(2) ptr: {:#x} cause: {}", ptr, e)
            }
        })?;
        Ok(())
    }
    pub async fn atomic_u32_check_async(
        &self,
        ptr: UserWritePtr<u32>,
        allocator: &mut dyn FrameAllocator,
    ) -> SysR<()> {
        let ptr = ptr.as_usize();
        match try_write_user_u32_atomic(ptr) {
            Ok(_) => return Ok(()),
            Err(e) => {
                if PRINT_CHECK_ERR {
                    println!("atomic_u32_check fail!(0) ptr: {:#x} cause: {}", ptr, e)
                }
            }
        }
        self.handle_write_fault_async(ptr, allocator)
            .await
            .inspect_err(|e| {
                if PRINT_CHECK_ERR {
                    println!("atomic_u32_check fail!(1) ptr: {:#x} cause: {}", ptr, e)
                }
            })?;
        try_write_user_u32_atomic(ptr).inspect_err(|e| {
            if PRINT_CHECK_ERR {
                println!("atomic_u32_check fail!(2) ptr: {:#x} cause: {}", ptr, e)
            }
        })?;
        Ok(())
    }
    #[inline(always)]
    pub fn read_check_only<T: Copy>(ptr: UserReadPtr<T>) -> SysR<()> {
        try_read_user_u8(ptr.as_usize()).map(|_| ())
    }
    #[inline(always)]
    pub fn write_check_only<T: Copy>(ptr: UserWritePtr<T>) -> SysR<()> {
        let value = try_read_user_u8(ptr.as_usize())?;
        try_write_user_u8(ptr.as_usize(), value).map(|_| ())
    }
    pub fn read_check_rough<T: Copy>(
        &self,
        ptr: UserReadPtr<T>,
        allocator: &mut dyn FrameAllocator,
    ) -> SysR<()> {
        let ptr = ptr.as_usize();
        match try_read_user_u8(ptr) {
            Ok(_v) => return Ok(()),
            Err(e) => {
                if PRINT_CHECK_ERR {
                    println!("read_check_rough fail!(0) ptr: {:#x} cause: {}", ptr, e)
                }
            }
        }
        self.handle_read_fault_rough(ptr, allocator)?;
        try_read_user_u8(ptr).inspect_err(|e| {
            if PRINT_CHECK_ERR {
                println!("read_check_rough fail!(1) ptr: {:#x} cause: {}", ptr, e)
            }
        })?;
        Ok(())
    }
    pub fn write_check_rough<T: Copy>(
        &self,
        ptr: UserWritePtr<T>,
        allocator: &mut dyn FrameAllocator,
    ) -> SysR<()> {
        let ptr = ptr.as_usize();
        let v = match try_read_user_u8(ptr) {
            Ok(v) => v,
            Err(_e) => {
                self.handle_write_fault_rough(ptr, allocator)
                    .inspect_err(|e| {
                        if PRINT_CHECK_ERR {
                            println!("write_check_rough fail!(0) ptr: {:#x} cause: {}", ptr, e)
                        }
                    })?;
                try_read_user_u8(ptr)?
            }
        };
        match try_write_user_u8(ptr, v) {
            Ok(()) => Ok(()),
            Err(_e) => {
                self.handle_write_fault_rough(ptr, allocator)
                    .inspect_err(|e| {
                        if PRINT_CHECK_ERR {
                            println!("write_check_rough fail!(1) ptr: {:#x} cause: {}", ptr, e)
                        }
                    })?;
                try_write_user_u8(ptr, v).inspect_err(|e| {
                    if PRINT_CHECK_ERR {
                        println!("write_check_rough fail!(2) ptr: {:#x} cause: {}", ptr, e)
                    }
                })?;
                Ok(())
            }
        }
    }
    pub async fn read_check_async<T: Copy>(
        &self,
        ptr: UserReadPtr<T>,
        allocator: &mut dyn FrameAllocator,
    ) -> SysR<()> {
        let ptr = ptr.as_usize();
        match try_read_user_u8(ptr) {
            Ok(_v) => return Ok(()),
            Err(e) => {
                if PRINT_CHECK_ERR {
                    println!("read_check_async fail!(0) ptr: {:#x} cause: {}", ptr, e)
                }
            }
        }
        self.handle_read_fault_async(ptr, allocator)
            .await
            .inspect_err(|e| {
                if PRINT_CHECK_ERR {
                    println!("read_check_async fail!(1) ptr: {:#x} cause: {}", ptr, e)
                }
            })?;
        try_read_user_u8(ptr).inspect_err(|e| {
            if PRINT_CHECK_ERR {
                println!("read_check_async fail!(2) ptr: {:#x} cause: {}", ptr, e)
            }
        })?;
        Ok(())
    }
    pub async fn write_check_async<T: Copy>(
        &self,
        ptr: UserWritePtr<T>,
        allocator: &mut dyn FrameAllocator,
    ) -> SysR<()> {
        let ptr = ptr.as_usize();
        let v = match try_read_user_u8(ptr) {
            Ok(v) => v,
            Err(_e) => {
                self.handle_write_fault_async(ptr, allocator)
                    .await
                    .inspect_err(|e| {
                        if PRINT_CHECK_ERR {
                            println!("write_check_async fail!(0) ptr: {:#x} cause: {}", ptr, e)
                        }
                    })?;
                try_read_user_u8(ptr).inspect_err(|e| {
                    if PRINT_CHECK_ERR {
                        println!("write_check_async fail!(1) ptr: {:#x} cause: {}", ptr, e)
                    }
                })?
            }
        };
        match try_write_user_u8(ptr, v) {
            Ok(_v) => return Ok(()),
            Err(_e) => {
                self.handle_write_fault_async(ptr, allocator)
                    .await
                    .inspect_err(|e| {
                        if PRINT_CHECK_ERR {
                            println!("try_write_user_u8 fail!(2) ptr: {:#x} cause: {}", ptr, e)
                        }
                    })?;
                try_write_user_u8(ptr, v).inspect_err(|e| {
                    if PRINT_CHECK_ERR {
                        println!("try_write_user_u8 fail!(3) ptr: {:#x} cause: {}", ptr, e)
                    }
                })?
            }
        }
        Ok(())
    }
    #[inline]
    fn handle_read_fault_rough(&self, ptr: usize, allocator: &mut dyn FrameAllocator) -> SysR<()> {
        if PRINT_PAGE_FAULT {
            println!(" handle_read_fault_rough {:#x}", ptr);
        }
        self.handle_fault_rough(ptr, AccessType::RO, allocator)
    }
    #[inline]
    fn handle_write_fault_rough(&self, ptr: usize, allocator: &mut dyn FrameAllocator) -> SysR<()> {
        if PRINT_PAGE_FAULT {
            println!("handle_write_fault_rough {:#x}", ptr);
        }
        self.handle_fault_rough(ptr, AccessType::RW, allocator)
    }
    #[inline]
    async fn handle_read_fault_async(
        &self,
        ptr: usize,
        allocator: &mut dyn FrameAllocator,
    ) -> SysR<()> {
        if PRINT_PAGE_FAULT {
            println!("handle_read_fault_async {:#x}", ptr);
        }
        self.handle_fault_async(ptr, AccessType::RO, allocator)
            .await
    }
    #[inline]
    async fn handle_write_fault_async(
        &self,
        ptr: usize,
        allocator: &mut dyn FrameAllocator,
    ) -> SysR<()> {
        if PRINT_PAGE_FAULT {
            println!("handle_write_fault_async {:#x}", ptr);
        }
        self.handle_fault_async(ptr, AccessType::RW, allocator)
            .await
    }
    /// 此函数只会处理简单的页错误, 例如简单的空间分配, 但无法处理文件映射
    ///
    /// 此函数不需要异步上下文!
    #[inline]
    fn handle_fault_rough(
        &self,
        ptr: usize,
        access: AccessType,
        allocator: &mut dyn FrameAllocator,
    ) -> SysR<()> {
        let ptr = UserAddr::try_from(ptr as *const u8)?.floor();
        let r = self
            .0
            .alive_then(move |a| a.user_space.map_segment.page_fault(ptr, access, allocator));
        match r {
            Ok(flush) => {
                flush.run();
                Ok(())
            }
            Err(TryRunFail::Error(e)) => Err(e),
            Err(TryRunFail::Async(_a)) => Err(SysError::EFAULT),
        }
    }
    /// 此函数处理完整的页错误并可能阻塞线程
    #[inline]
    async fn handle_fault_async(
        &self,
        ptr: usize,
        access: AccessType,
        allocator: &mut dyn FrameAllocator,
    ) -> SysR<()> {
        let ptr = UserAddr::try_from(ptr as *const u8)?.floor();
        let r = self
            .0
            .alive_then(move |a| a.user_space.map_segment.page_fault(ptr, access, allocator));
        let a = match r {
            Ok(flush) => {
                flush.run();
                return Ok(());
            }
            Err(TryRunFail::Error(e)) => return Err(e),
            Err(TryRunFail::Async(a)) => a,
        };
        unsafe { trap::set_kernel_default_trap() };
        match a.a_page_fault(self.0, ptr).await {
            Ok(flush) => {
                flush.run();
                unsafe { set_error_handle() };
                Ok(())
            }
            Err(e) => {
                unsafe { set_error_handle() };
                Err(e)
            }
        }
    }
}

#[inline]
fn try_read_user_u8(ptr: usize) -> SysR<u8> {
    #[allow(improper_ctypes)]
    extern "C" {
        /// return false if success, return true if error.
        ///
        /// if return Err, cause must be Exception::LoadPageFault
        fn __try_read_user_u8(ptr: usize) -> (usize, usize);
    }
    let (flag, value) = unsafe { __try_read_user_u8(ptr) };
    match flag {
        0 => Ok(value as u8),
        _ => {
            if cfg!(debug_assertions) {
                let scause: Scause = unsafe { core::mem::transmute(value) };
                match scause.cause() {
                    scause::Trap::Interrupt(i) => unreachable!("{:?}", i),
                    scause::Trap::Exception(e) => assert_eq!(e, Exception::LoadPageFault),
                };
            }
            Err(SysError::EFAULT)
        }
    }
}

#[inline]
fn try_write_user_u8(ptr: usize, value: u8) -> SysR<()> {
    #[allow(improper_ctypes)]
    extern "C" {
        /// return false if success, return true if error.
        ///
        /// if return Err, scause must be Exception::StorePageFault
        fn __try_write_user_u8(ptr: usize, value: u8) -> (usize, usize);
    }
    let (flag, value) = unsafe { __try_write_user_u8(ptr, value) };
    match flag {
        0 => Ok(()),
        _ => {
            if cfg!(debug_assertions) {
                let scause: Scause = unsafe { core::mem::transmute(value) };
                match scause.cause() {
                    scause::Trap::Interrupt(i) => unreachable!("{:?}", i),
                    scause::Trap::Exception(e) => assert_eq!(e, Exception::StorePageFault),
                };
            }
            Err(SysError::EFAULT)
        }
    }
}

/// 使用 ptr.fetch_add(0) 来测试写权限并保证不修改值
#[inline]
fn try_write_user_u32_atomic(ptr: usize) -> SysR<()> {
    #[allow(improper_ctypes)]
    extern "C" {
        /// amoadd.d a1, zero, (a1)
        fn __try_write_user_u32_atomic(ptr: usize) -> (usize, usize);
    }
    let (flag, value) = unsafe { __try_write_user_u32_atomic(ptr) };
    match flag {
        0 => Ok(()),
        _ => {
            if cfg!(debug_assertions) {
                let scause: Scause = unsafe { core::mem::transmute(value) };
                match scause.cause() {
                    scause::Trap::Interrupt(i) => unreachable!("{:?}", i),
                    scause::Trap::Exception(e) => assert_eq!(e, Exception::StorePageFault),
                };
            }
            Err(SysError::EFAULT)
        }
    }
}

#[inline]
unsafe fn set_error_handle() {
    extern "C" {
        fn __try_access_user_error_trap();
        fn __try_access_user_error_vector();
    }
    // debug_assert!(!sstatus::read().sie());
    if true {
        // 向量模式, 速度更快
        stvec::write(__try_access_user_error_vector as usize, TrapMode::Vectored);
    } else {
        // 直接跳转模式, 在handle中处理中断
        stvec::write(__try_access_user_error_trap as usize, TrapMode::Direct);
    }
}

/// 将陷阱函数设置为用户态检测句柄
pub(super) struct NativeErrorHandle;
impl Drop for NativeErrorHandle {
    fn drop(&mut self) {
        unsafe { trap::set_kernel_default_trap() };
    }
}
impl NativeErrorHandle {
    pub unsafe fn new() -> Self {
        set_error_handle();
        Self
    }
}

// 只有check_impl.S的两个函数可以进入这里, 中断会丢失寄存器信息
#[no_mangle]
fn try_access_user_error_debug() {
    let cause = scause::read().cause();
    let stval = stval::read();
    let sepc = sepc::read();
    println!("cause {:?} stval {:#x} sepc {:#x}", cause, stval, sepc);
    match cause {
        scause::Trap::Exception(Exception::LoadPageFault | Exception::StorePageFault) => (),
        // if handle this must save all register!!!
        // scause::Trap::Interrupt(i) if i == Interrupt::SupervisorTimer => {
        //     panic!("{:#x}", sepc::read())
        // }
        x => panic!("{:?}", x),
    }
}
