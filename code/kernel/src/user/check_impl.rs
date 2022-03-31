use core::arch::global_asm;

use riscv::register::{
    scause::{self, Exception, Scause},
    sepc, stval, stvec,
    utvec::TrapMode,
};

use crate::{
    local,
    memory::user_ptr::{UserReadPtr, UserWritePtr},
    syscall::{SysError, UniqueSysError},
    trap,
    xdebug::PRINT_PAGE_FAULT,
};

use super::{UserAccessStatus, UserType};

global_asm!(include_str!("check_impl.S"));

pub(super) struct UserCheckImpl;

impl Drop for UserCheckImpl {
    fn drop(&mut self) {
        unsafe { trap::set_kernel_default_trap() };
        assert!(Self::status().is_access());
    }
}

impl UserCheckImpl {
    pub fn new() -> Self {
        assert!(Self::status().is_access());
        unsafe { set_error_handle() };
        Self
    }
    fn status() -> &'static mut UserAccessStatus {
        &mut local::always_local().user_access_status
    }
    pub async fn read_check<T: UserType>(
        &self,
        ptr: UserReadPtr<T>,
    ) -> Result<(), UniqueSysError<{ SysError::EFAULT as isize }>> {
        let ptr = ptr.as_usize();
        match try_read_user_u8(ptr) {
            Ok(_v) => return Ok(()),
            Err(_e) => (),
        }
        Self::handle_read_fault(ptr).await?;
        try_read_user_u8(ptr)?;
        Ok(())
    }
    pub async fn write_check<T: UserType>(
        &self,
        ptr: UserWritePtr<T>,
    ) -> Result<(), UniqueSysError<{ SysError::EFAULT as isize }>> {
        // let ptr = ptr.raw_ptr_mut() as *mut u8;
        let ptr = ptr.as_usize();
        let v = match try_read_user_u8(ptr) {
            Ok(v) => v,
            Err(_e) => {
                Self::handle_write_fault(ptr).await?;
                try_read_user_u8(ptr)?
            }
        };
        match try_write_user_u8(ptr, v) {
            Ok(_v) => return Ok(()),
            Err(_e) => {
                Self::handle_write_fault(ptr).await?;
                try_write_user_u8(ptr, v)?
            }
        }
        Ok(())
    }
    async fn handle_read_fault(ptr: usize) -> Result<(), ()> {
        if PRINT_PAGE_FAULT {
            println!(" read fault of {:#x}", ptr);
        }
        Err(())
    }
    async fn handle_write_fault(ptr: usize) -> Result<(), ()> {
        if PRINT_PAGE_FAULT {
            println!("write fault of {:#x}", ptr);
        }
        Err(())
    }
}

/// return false if success, return true if error.
///
/// if return Err, cause must be Exception::LoadPageFault
fn try_read_user_u8(ptr: usize) -> Result<u8, ()> {
    #[allow(improper_ctypes)]
    extern "C" {
        fn __try_read_user_u8(ptr: usize) -> (usize, usize);
    }
    let (flag, value) = unsafe { __try_read_user_u8(ptr) };
    match flag {
        0 => Ok(value as u8),
        _ => {
            let scause: Scause = unsafe { core::mem::transmute(value) };
            match scause.cause() {
                scause::Trap::Interrupt(i) => unreachable!("{:?}", i),
                scause::Trap::Exception(e) => assert_eq!(e, Exception::LoadPageFault),
            };
            Err(())
        }
    }
}

/// return false if success, return true if error.
///
/// if return Err, cause must be Exception::StorePageFault
fn try_write_user_u8(ptr: usize, value: u8) -> Result<(), ()> {
    #[allow(improper_ctypes)]
    extern "C" {
        fn __try_write_user_u8(ptr: usize, value: u8) -> (usize, usize);
    }
    let (flag, value) = unsafe { __try_write_user_u8(ptr, value) };
    match flag {
        0 => Ok(()),
        _ => {
            let scause: Scause = unsafe { core::mem::transmute(value) };
            match scause.cause() {
                scause::Trap::Interrupt(i) => unreachable!("{:?}", i),
                scause::Trap::Exception(e) => assert_eq!(e, Exception::StorePageFault),
            };
            Err(())
        }
    }
}

unsafe fn set_error_handle() {
    extern "C" {
        fn __try_access_user_error_vector();
    }
    stvec::write(__try_access_user_error_vector as usize, TrapMode::Vectored);
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
