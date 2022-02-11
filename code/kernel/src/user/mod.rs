use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::{convert::TryFrom, ptr, str::Utf8Error};

use crate::{riscv::register::sstatus, trap::context::TrapContext};

use crate::memory::address::{OutOfUserRange, PageCount, UserAddr};

pub struct UserData {
    data: *const [u8],
}

pub struct UserDataGuard<'a> {
    data: &'static [u8],
    _auto_sum: AutoSum,
    _mark: PhantomData<&'a ()>,
}
impl<'a> Deref for UserDataGuard<'a> {
    type Target = [u8];

    fn deref(&self) -> &'a Self::Target {
        self.data
    }
}
pub struct UserDataGuardMut<'a> {
    data: *mut [u8],
    _auto_sum: AutoSum,
    _mark: PhantomData<&'a ()>,
}
impl<'a> Deref for UserDataGuardMut<'a> {
    type Target = [u8];

    fn deref(&self) -> &'a Self::Target {
        unsafe { &*self.data }
    }
}
impl<'a> DerefMut for UserDataGuardMut<'a> {
    fn deref_mut(&mut self) -> &'a mut Self::Target {
        unsafe { &mut *self.data }
    }
}
impl UserData {
    pub fn new(data: *const [u8]) -> Self {
        Self { data }
    }
    pub fn access(&self) -> UserDataGuard {
        UserDataGuard {
            data: unsafe { &*self.data },
            _auto_sum: AutoSum::new(),
            _mark: PhantomData
        }
    }
}
pub struct UserDataMut {
    data: *mut [u8],
}

impl UserDataMut {
    pub fn new(data: *mut [u8]) -> Self {
        Self { data }
    }
    pub fn access(&self) -> UserDataGuard {
        UserDataGuard {
            data: unsafe { &*self.data },
            _auto_sum: AutoSum::new(),
            _mark: PhantomData
        }
    }
    pub fn access_mut(&self) -> UserDataGuardMut {
        UserDataGuardMut {
            data: unsafe { &mut *self.data },
            _auto_sum: AutoSum::new(),
            _mark: PhantomData
        }
    }
}

/// read in volatile
#[derive(Debug)]
pub enum UserAccessStatus {
    Forbid,
    Access,
    Error, // set by interrupt
}

#[derive(Debug)]
pub enum UserAccessError {
    OutOfUserRange(OutOfUserRange),
    AccessError(UserAccessStatus),
}

impl From<OutOfUserRange> for UserAccessError {
    fn from(s: OutOfUserRange) -> Self {
        Self::OutOfUserRange(s)
    }
}

impl From<UserAccessStatus> for UserAccessError {
    fn from(s: UserAccessStatus) -> Self {
        Self::AccessError(s)
    }
}

impl UserAccessStatus {
    pub fn set_forbit(&mut self) {
        *self = UserAccessStatus::Forbid;
    }
    pub fn set_access(&mut self) {
        *self = UserAccessStatus::Access;
    }
    pub fn is_forbid_volatile(&self) -> bool {
        let x = unsafe { ptr::read_volatile(self) };
        matches!(x, UserAccessStatus::Forbid)
    }
    pub fn not_forbid_volatile(&self) -> bool {
        !self.is_forbid_volatile()
    }
    pub fn is_access_volatile(&self) -> bool {
        let x = unsafe { ptr::read_volatile(self) };
        matches!(x, UserAccessStatus::Access)
    }
    pub fn is_error_volatile(&self) -> bool {
        let x = unsafe { ptr::read_volatile(self) };
        matches!(x, UserAccessStatus::Error)
    }
    pub fn access_check(&self) -> Result<(), UserAccessError> {
        let x = unsafe { ptr::read_volatile(self) };
        match x {
            UserAccessStatus::Access => Ok(()),
            e => Err(e.into()),
        }
    }
}

pub struct AutoSum;
impl AutoSum {
    pub fn new() -> Self {
        unsafe { sstatus::set_sum() };
        Self
    }
}
impl Drop for AutoSum {
    fn drop(&mut self) {
        unsafe { sstatus::clear_sum() }
    }
}
pub struct TraceAutoSum(*mut UserAccessStatus, AutoSum);
impl TraceAutoSum {
    pub fn new(trap_context: &mut TrapContext) -> Self {
        let status = &mut trap_context.user_access_status;
        assert!(status.is_forbid_volatile());
        status.set_access();
        Self(&mut trap_context.user_access_status, AutoSum::new())
    }
}

impl Drop for TraceAutoSum {
    fn drop(&mut self) {
        unsafe {
            let status = &mut (*self.0);
            assert!(status.not_forbid_volatile());
            status.set_forbit();
        }
    }
}

#[derive(Debug)]
pub enum UserAccessU8Error {
    OutOfUserRange(OutOfUserRange),
    UserAccessError(UserAccessError),
}
impl From<OutOfUserRange> for UserAccessU8Error {
    fn from(e: OutOfUserRange) -> Self {
        Self::OutOfUserRange(e)
    }
}
impl From<UserAccessError> for UserAccessU8Error {
    fn from(e: UserAccessError) -> Self {
        Self::UserAccessError(e)
    }
}

pub fn translated_user_u8(
    trap_context: &TrapContext,
    ptr: *const u8,
) -> Result<u8, UserAccessU8Error> {
    let uptr = UserAddr::try_from(ptr)?;
    let value = *uptr.get_mut();
    trap_context.user_access_status.access_check()?;
    Ok(value)
}

#[derive(Debug)]
pub enum UserAccessStrError {
    OutOfUserRange(OutOfUserRange),
    UserAccessError(UserAccessError),
    Utf8Error(Utf8Error),
}
impl From<OutOfUserRange> for UserAccessStrError {
    fn from(e: OutOfUserRange) -> Self {
        Self::OutOfUserRange(e)
    }
}
impl From<UserAccessError> for UserAccessStrError {
    fn from(e: UserAccessError) -> Self {
        Self::UserAccessError(e)
    }
}
impl From<UserAccessU8Error> for UserAccessStrError {
    fn from(e: UserAccessU8Error) -> Self {
        match e {
            UserAccessU8Error::OutOfUserRange(e) => Self::OutOfUserRange(e),
            UserAccessU8Error::UserAccessError(e) => Self::UserAccessError(e),
        }
    }
}
impl From<Utf8Error> for UserAccessStrError {
    fn from(e: Utf8Error) -> Self {
        Self::Utf8Error(e)
    }
}

pub fn translated_user_str_zero_end(
    trap_context: &mut TrapContext,
    ptr: *const u8,
) -> Result<UserData, UserAccessStrError> {
    let mut uptr = UserAddr::try_from(ptr)?;
    let _trace = TraceAutoSum::new(trap_context);
    let mut len = 0;
    let mut get_ch = || {
        let ch: u8 = unsafe { *uptr.as_ptr() }; // if access fault, return 0.
        uptr.add_assign(1);
        (ch, uptr)
    };
    let (ch, _next_ptr) = get_ch();
    // check first access
    trap_context.user_access_status.access_check()?;
    if ch != 0 {
        len += 1;
    } else {
        let slice = unsafe { &*ptr::slice_from_raw_parts(ptr, 0) };
        return Ok(UserData::new(slice));
    }
    loop {
        let (ch, next_ptr) = get_ch();
        if ch == 0 {
            break;
        }
        len += 1;
        // check when first access a page.
        if next_ptr.page_offset() == 1 {
            trap_context.user_access_status.access_check()?;
        }
    }
    trap_context.user_access_status.access_check()?;
    let slice = unsafe { &*ptr::slice_from_raw_parts(ptr, len) };
    return Ok(UserData::new(slice));
}

pub fn translated_user_read_range(
    trap_context: &mut TrapContext,
    ptr: *const u8,
    len: usize,
) -> Result<UserData, UserAccessStrError> {
    let ubegin = UserAddr::try_from(ptr)?;
    let uend = UserAddr::try_from((ptr as usize + len) as *const u8)?;
    let _trace = TraceAutoSum::new(trap_context);
    let mut cur = ubegin.floor();
    let uend4k = uend.ceil();
    while cur != uend4k {
        let cur_ptr = cur.into_usize() as *const u8;
        let _v = unsafe { cur_ptr.read_volatile() };
        trap_context.user_access_status.access_check()?;
        cur.add_page_assign(PageCount::from_usize(1));
    }
    let slice = ptr::slice_from_raw_parts(ptr, len);
    Ok(UserData::new(unsafe { &*slice }))
}

pub fn translated_user_write_range(
    trap_context: &mut TrapContext,
    ptr: *mut u8,
    len: usize,
) -> Result<UserDataMut, UserAccessError> {
    let ubegin = UserAddr::try_from(ptr)?;
    let uend = UserAddr::try_from((ptr as usize + len) as *mut u8)?;
    let _trace = TraceAutoSum::new(trap_context);
    let mut cur = ubegin.floor();
    let uend4k = uend.ceil();
    while cur != uend4k {
        let cur_ptr = cur.into_usize() as *mut u8;
        unsafe {
            let v = cur_ptr.read_volatile();
            cur_ptr.write_volatile(v);
        }
        trap_context.user_access_status.access_check()?;
        cur.add_page_assign(PageCount::from_usize(1));
    }
    let slice = ptr::slice_from_raw_parts_mut(ptr, len);
    Ok(UserDataMut::new(slice))
}
