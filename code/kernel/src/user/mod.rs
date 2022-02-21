use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::{convert::TryFrom, ptr};

use alloc::vec::Vec;

use crate::local;
use crate::riscv::register::sstatus;

use crate::memory::address::{OutOfUserRange, PageCount, UserAddr};
use crate::syscall::{SysError, UniqueSysError};

pub struct UserData<T: 'static> {
    data: *const [T],
}

unsafe impl<T: 'static> Send for UserData<T> {}
unsafe impl<T: 'static> Sync for UserData<T> {}

pub struct UserDataGuard<'a, T: 'static> {
    data: &'static [T],
    _auto_sum: AutoSum,
    _mark: PhantomData<&'a ()>,
}

unsafe impl<T: 'static> Send for UserDataGuard<'_, T> {}
unsafe impl<T: 'static> Sync for UserDataGuard<'_, T> {}

impl<'a, T> Deref for UserDataGuard<'a, T> {
    type Target = [T];

    fn deref(&self) -> &'a Self::Target {
        self.data
    }
}
pub struct UserDataGuardMut<'a, T> {
    data: *mut [T],
    _auto_sum: AutoSum,
    _mark: PhantomData<&'a ()>,
}

unsafe impl<T: 'static> Send for UserDataGuardMut<'_, T> {}
unsafe impl<T: 'static> Sync for UserDataGuardMut<'_, T> {}

impl<'a, T> Deref for UserDataGuardMut<'a, T> {
    type Target = [T];

    fn deref(&self) -> &'a Self::Target {
        unsafe { &*self.data }
    }
}
impl<'a, T> DerefMut for UserDataGuardMut<'a, T> {
    fn deref_mut(&mut self) -> &'a mut Self::Target {
        unsafe { &mut *self.data }
    }
}
impl<T: 'static> UserData<T> {
    pub fn new(data: *const [T]) -> Self {
        Self { data }
    }
    pub fn access(&self) -> UserDataGuard<'_, T> {
        UserDataGuard {
            data: unsafe { &*self.data },
            _auto_sum: AutoSum::new(),
            _mark: PhantomData,
        }
    }
}
impl<T: Clone + 'static> UserData<T> {
    pub fn into_vec(&self) -> Vec<T> {
        self.access().to_vec()
    }
}
pub struct UserDataMut<T> {
    data: *mut [T],
}

unsafe impl<T: 'static> Send for UserDataMut<T> {}
unsafe impl<T: 'static> Sync for UserDataMut<T> {}


impl<T> UserDataMut<T> {
    pub fn new(data: *mut [T]) -> Self {
        Self { data }
    }
    pub fn access(&self) -> UserDataGuard<'_, T> {
        UserDataGuard {
            data: unsafe { &*self.data },
            _auto_sum: AutoSum::new(),
            _mark: PhantomData,
        }
    }
    pub fn access_mut(&self) -> UserDataGuardMut<T> {
        UserDataGuardMut {
            data: unsafe { &mut *self.data },
            _auto_sum: AutoSum::new(),
            _mark: PhantomData,
        }
    }
}
impl<T: Clone + 'static> UserDataMut<T> {
    pub fn into_vec(&self) -> Vec<T> {
        self.access().to_vec()
    }
}

/// read in volatile, need register in core
#[derive(Debug, Clone, Copy)]
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
    pub fn access_check(&self) -> Result<(), UniqueSysError<{ SysError::EFAULT as isize }>> {
        let x = unsafe { ptr::read_volatile(self) };
        match x {
            UserAccessStatus::Access => Ok(()),
            _e => Err(UniqueSysError),
        }
    }
}

pub trait UserType: Copy + 'static {
    fn is_null(&self) -> bool;
}
macro_rules! user_type_impl_default {
    ($type: ident) => {
        impl UserType for $type {
            fn is_null(&self) -> bool {
                *self == 0
            }
        }
    };
}
user_type_impl_default!(usize);
user_type_impl_default!(isize);
user_type_impl_default!(u64);
user_type_impl_default!(i64);
user_type_impl_default!(u32);
user_type_impl_default!(i32);
user_type_impl_default!(u16);
user_type_impl_default!(i16);
user_type_impl_default!(u8);
user_type_impl_default!(i8);
impl<T: 'static> UserType for *const T {
    fn is_null(&self) -> bool {
        *self == core::ptr::null()
    }
}
impl<T: 'static> UserType for *mut T {
    fn is_null(&self) -> bool {
        *self == core::ptr::null_mut()
    }
}

pub struct AutoSie(bool);
impl AutoSie {
    pub fn new() -> Self {
        let f = sstatus::read().sie();
        unsafe { sstatus::set_sie() };
        Self(f)
    }
}

impl Drop for AutoSie {
    fn drop(&mut self) {
        if !self.0 {
            unsafe { sstatus::clear_sie() }
        }
    }
}

/// access user data and close interrupt.
pub struct AutoSum(bool, AutoSie);
impl AutoSum {
    pub fn new() -> Self {
        let f = sstatus::read().sum();
        let sie = AutoSie::new();
        unsafe { sstatus::set_sum() };
        Self(f, sie)
    }
}

impl Drop for AutoSum {
    fn drop(&mut self) {
        if !self.0 {
            unsafe { sstatus::clear_sum() }
            // clear sie later
        }
    }
}

pub struct UserAccessTrace(*mut UserAccessStatus, AutoSum);
impl UserAccessTrace {
    pub fn new(user_access_status: &mut UserAccessStatus) -> Self {
        assert!(user_access_status.is_forbid_volatile());
        user_access_status.set_access();
        Self(user_access_status, AutoSum::new())
    }
}

impl Drop for UserAccessTrace {
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
    ptr: *const u8,
) -> Result<u8, UniqueSysError<{ SysError::EFAULT as isize }>> {
    let uptr = UserAddr::try_from(ptr)?;
    let user_access_status = &mut local::current_local().user_access_status;
    let value = *uptr.get_mut();
    user_access_status.access_check()?;
    Ok(value)
}

pub fn translated_user_array_zero_end<T>(
    ptr: *const T,
) -> Result<UserData<T>, UniqueSysError<{ SysError::EFAULT as isize }>>
where
    T: UserType,
{
    let mut uptr = UserAddr::try_from(ptr)?;
    let user_access_status = &mut local::current_local().user_access_status;
    let _trace = UserAccessTrace::new(user_access_status);
    let mut len = 0;
    let mut get_ch = || {
        let ch: T = unsafe { *uptr.as_ptr() }; // if access fault, return 0.
        uptr.add_assign(1);
        (ch, uptr)
    };
    let (ch, _next_ptr) = get_ch();
    // check first access
    user_access_status.access_check()?;
    if !ch.is_null() {
        len += 1;
    } else {
        let slice = unsafe { &*ptr::slice_from_raw_parts(ptr, 0) };
        return Ok(UserData::new(slice));
    }
    loop {
        let (ch, next_ptr) = get_ch();
        if ch.is_null() {
            break;
        }
        len += 1;
        // check when first access a page.
        if next_ptr.page_offset() == core::mem::size_of::<T>() {
            user_access_status.access_check()?;
        }
    }
    user_access_status.access_check()?;
    let slice = unsafe { &*ptr::slice_from_raw_parts(ptr, len) };
    return Ok(UserData::new(slice));
}

pub fn translated_user_2d_array_zero_end<T>(
    ptr: *const *const T,
) -> Result<Vec<UserData<T>>, UniqueSysError<{ SysError::EFAULT as isize }>>
where
    T: UserType,
{
    let arr_1d = translated_user_array_zero_end(ptr)?;
    let mut ret = Vec::new();
    for &arr_2d in &*arr_1d.access() {
        ret.push(translated_user_array_zero_end(arr_2d)?);
    }
    Ok(ret)
}

pub fn translated_user_readonly_slice<T>(
    ptr: *const T,
    len: usize,
) -> Result<UserData<T>, UniqueSysError<{ SysError::EFAULT as isize }>> {
    let ubegin = UserAddr::try_from(ptr)?;
    let uend = UserAddr::try_from((ptr as usize + len) as *const u8)?;
    let user_access_status = &mut local::current_local().user_access_status;
    let trace = UserAccessTrace::new(user_access_status);
    let mut cur = ubegin.floor();
    let uend4k = uend.ceil();
    while cur != uend4k {
        let cur_ptr = cur.into_usize() as *const u8;
        // if error occur will change status by exception
        let _v = unsafe { cur_ptr.read_volatile() };
        user_access_status.access_check()?;
        cur.add_page_assign(PageCount::from_usize(1));
    }
    drop(trace);
    let slice = ptr::slice_from_raw_parts(ptr, len);
    Ok(UserData::new(unsafe { &*slice }))
}

pub fn translated_user_writable_slice<T>(
    ptr: *mut T,
    len: usize,
) -> Result<UserDataMut<T>, UniqueSysError<{ SysError::EFAULT as isize }>> {
    let ubegin = UserAddr::try_from(ptr)?;
    let uend = UserAddr::try_from((ptr as usize + len) as *mut u8)?;
    let user_access_status = &mut local::current_local().user_access_status;
    let trace = UserAccessTrace::new(user_access_status);
    let mut cur = ubegin.floor();
    let uend4k = uend.ceil();
    while cur != uend4k {
        let cur_ptr = cur.into_usize() as *mut u8;
        unsafe {
            // if error occur will change status by exception
            let v = cur_ptr.read_volatile();
            cur_ptr.write_volatile(v);
        }
        local::current_local().user_access_status.access_check()?;
        cur.add_page_assign(PageCount::from_usize(1));
    }
    drop(trace);
    let slice = ptr::slice_from_raw_parts_mut(ptr, len);
    Ok(UserDataMut::new(slice))
}
