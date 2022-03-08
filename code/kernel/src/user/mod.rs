use core::ops::{Deref, DerefMut};
use core::{marker::PhantomData, ptr};

use alloc::vec::Vec;

use crate::local;
use crate::riscv::register::sstatus;
use crate::{
    memory::{address::OutOfUserRange, allocator::frame::global::FrameTracker},
    syscall::{SysError, UniqueSysError},
};

use self::iter::{UserData4KIter, UserDataMut4KIter};

pub mod check;
pub mod iter;

pub struct UserData<T: 'static> {
    data: *const [T],
}

unsafe impl<T: 'static> Send for UserData<T> {}
unsafe impl<T: 'static> Sync for UserData<T> {}

pub struct UserDataGuard<'a, T: 'static> {
    data: &'static [T],
    _mark: PhantomData<&'a ()>,
    _auto_sum: AutoSum,
}

impl<'a, T> !Send for UserDataGuard<'a, T> {}
impl<'a, T> !Sync for UserDataGuard<'a, T> {}

// unsafe impl<T: 'static> Send for UserDataGuard<'_, T> {}
// unsafe impl<T: 'static> Sync for UserDataGuard<'_, T> {}

impl<'a, T: 'static> Deref for UserDataGuard<'a, T> {
    type Target = [T];

    fn deref(&self) -> &'a Self::Target {
        self.data
    }
}
pub struct UserDataGuardMut<'a, T: 'static> {
    data: &'static mut [T],
    _mark: PhantomData<&'a ()>,
    _auto_sum: AutoSum,
}

// unsafe impl<T: 'static> Send for UserDataGuardMut<'_, T> {}
// unsafe impl<T: 'static> Sync for UserDataGuardMut<'_, T> {}

impl<'a, T: 'static> Deref for UserDataGuardMut<'a, T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.data
    }
}
impl<'a, T: 'static> DerefMut for UserDataGuardMut<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.data
    }
}

impl<T: 'static> UserData<T> {
    pub fn new(data: *const [T]) -> Self {
        Self { data }
    }
    pub fn len(&self) -> usize {
        self.data.len()
    }
    pub fn access(&self) -> UserDataGuard<T> {
        UserDataGuard {
            data: unsafe { &*self.data },
            _mark: PhantomData,
            _auto_sum: AutoSum::new(),
        }
    }
}

impl UserData<u8> {
    /// return an read only iterator containing a 4KB buffer.
    ///
    /// before each access, it will copy 4KB from user range to buffer.
    pub fn read_only_iter(&self, buffer: FrameTracker) -> UserData4KIter {
        UserData4KIter::new(self, buffer)
    }
}

impl<T: Clone + 'static> UserData<T> {
    /// after into_vec the data will no longer need space_guard.
    pub fn into_vec(&self) -> Vec<T> {
        self.access().to_vec()
    }
}

pub struct UserDataMut<T: 'static> {
    data: *mut [T],
}

unsafe impl<T: 'static> Send for UserDataMut<T> {}
unsafe impl<T: 'static> Sync for UserDataMut<T> {}

impl<T> UserDataMut<T> {
    pub fn new(data: *mut [T]) -> Self {
        Self { data }
    }
    pub fn len(&self) -> usize {
        self.data.len()
    }
    pub fn access<'b>(&self) -> UserDataGuard<'b, T> {
        UserDataGuard {
            data: unsafe { &*self.data },
            _mark: PhantomData,
            _auto_sum: AutoSum::new(),
        }
    }
    pub fn access_mut<'b>(&self) -> UserDataGuardMut<'b, T> {
        UserDataGuardMut {
            data: unsafe { &mut *self.data },
            _mark: PhantomData,
            _auto_sum: AutoSum::new(),
        }
    }
    pub fn as_const(&self) -> &UserData<T> {
        unsafe { core::mem::transmute(self) }
    }
}

impl UserDataMut<u8> {
    /// return an read only iterator containing a 4KB buffer.
    ///
    /// before each access, it will copy 4KB from user range to buffer.
    pub fn read_only_iter(&self, buffer: FrameTracker) -> UserData4KIter {
        UserData4KIter::new(self.as_const(), buffer)
    }
    /// return an write only iterator containing a 4KB buffer.
    ///
    /// before each access, it will copy 4KB from buffer to user range except for the first time.
    pub fn write_only_iter(&self, buffer: FrameTracker) -> UserDataMut4KIter {
        UserDataMut4KIter::new(self, buffer)
    }
}

impl<T: Clone + 'static> UserDataMut<T> {
    /// after into_vec the data will no longer need space_guard.
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

pub struct AutoSie;

impl !Send for AutoSie {}
impl !Sync for AutoSie {}

impl AutoSie {
    pub fn new() -> Self {
        local::current_local().sie_inc();
        Self
    }
}

impl Drop for AutoSie {
    fn drop(&mut self) {
        local::current_local().sie_dec();
    }
}

/// access user data and close interrupt.
pub struct AutoSum(AutoSie);

impl !Send for AutoSum {}
impl !Sync for AutoSum {}

impl AutoSum {
    pub fn new() -> Self {
        let sie = AutoSie::new();
        local::current_local().sum_inc();
        Self(sie)
    }
}

impl Drop for AutoSum {
    fn drop(&mut self) {
        local::current_local().sum_dec();
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
