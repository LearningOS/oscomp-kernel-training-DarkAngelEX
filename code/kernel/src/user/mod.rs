use core::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
    ptr,
};

use alloc::vec::Vec;
use riscv::register::scause::Exception;

use crate::{
    executor, local,
    memory::{
        self,
        address::{OutOfUserRange, UserAddr},
        allocator::frame::global::FrameTracker,
        user_ptr::{Policy, UserPtr},
        PTEFlags, UserSpace,
    },
    process::proc_table,
    user::check_impl::UserCheckImpl,
};

use self::iter::{UserData4KIter, UserDataMut4KIter};

pub mod check;
mod check_impl;
pub mod iter;
pub mod trap_handler;

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

unsafe impl<T: 'static> Send for UserDataGuard<'_, T> {}
unsafe impl<T: 'static> Sync for UserDataGuard<'_, T> {}

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

unsafe impl<T: 'static> Send for UserDataGuardMut<'_, T> {}
unsafe impl<T: 'static> Sync for UserDataGuardMut<'_, T> {}

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
    /// after to_vec the data will no longer need space_guard.
    pub fn to_vec(&self) -> Vec<T> {
        self.access().to_vec()
    }
    pub fn load(&self) -> T {
        debug_assert_eq!(self.data.len(), 1);
        let _sum = AutoSum::new();
        unsafe { (*self.data)[0].clone() }
    }
}

pub struct UserDataMut<T: 'static> {
    data: *mut [T],
}

unsafe impl<T: 'static> Send for UserDataMut<T> {}
unsafe impl<T: 'static> Sync for UserDataMut<T> {}

#[allow(dead_code)]
impl<T> UserDataMut<T> {
    pub fn new(data: *mut [T]) -> Self {
        Self { data }
    }
    pub fn len(&self) -> usize {
        self.data.len()
    }
    pub fn access(&self) -> UserDataGuard<'_, T> {
        UserDataGuard {
            data: unsafe { &*self.data },
            _mark: PhantomData,
            _auto_sum: AutoSum::new(),
        }
    }
    pub fn access_mut(&self) -> UserDataGuardMut<'_, T> {
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

#[allow(dead_code)]
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

#[allow(dead_code)]
impl<T: Clone + 'static> UserDataMut<T> {
    /// after to_vec the data will no longer need space_guard.
    pub fn to_vec(&self) -> Vec<T> {
        self.access().to_vec()
    }
    pub fn load(&self) -> T {
        debug_assert_eq!(self.data.len(), 1);
        let _sum = AutoSum::new();
        unsafe { (*self.data)[0].clone() }
    }
    pub fn store(&self, v: T) {
        debug_assert_eq!(self.data.len(), 1);
        let _sum = AutoSum::new();
        unsafe { (*self.data)[0] = v }
    }
}

/// read in volatile, need register in core
#[derive(Debug, Clone, Copy)]
pub enum UserAccessStatus {
    Forbid,
    Access,
    Error(UserAddr, Exception), // stval, Excetion
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
    pub fn get(&self) -> Self {
        unsafe { ptr::read_volatile(self) }
    }
    pub fn set(&mut self, value: Self) {
        unsafe { ptr::write_volatile(self, value) };
    }
    pub fn set_forbid(&mut self) {
        self.set(UserAccessStatus::Forbid);
    }
    pub fn set_access(&mut self) {
        self.set(UserAccessStatus::Access);
    }
    pub fn is_forbid(&self) -> bool {
        matches!(self.get(), UserAccessStatus::Forbid)
    }
    pub fn not_forbid(&self) -> bool {
        !self.is_forbid()
    }
    pub fn is_access(&self) -> bool {
        matches!(self.get(), UserAccessStatus::Access)
    }
    pub fn set_error(&mut self, stval: UserAddr, e: Exception) {
        self.set(UserAccessStatus::Error(stval, e))
    }
}

pub trait UserType: Copy + Send + 'static {
    fn is_null(&self) -> bool;
    fn new_usize(a: usize) -> Self;
}
macro_rules! user_type_impl_default {
    ($type: ident) => {
        impl UserType for $type {
            fn is_null(&self) -> bool {
                *self == 0
            }
            fn new_usize(a: usize) -> $type {
                a as $type
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
impl<T: Clone + Copy + 'static, P: Policy + 'static> UserType for UserPtr<T, P> {
    fn is_null(&self) -> bool {
        self.raw_ptr().is_null()
    }
    fn new_usize(a: usize) -> Self {
        Self::from_usize(a)
    }
}
// impl<T: 'static> UserType for *mut T {
//     fn is_null(&self) -> bool {
//         *self == core::ptr::null_mut()
//     }
//     fn new_usize(a: usize) -> Self {
//         a as Self
//     }
// }

pub struct AutoSie;

unsafe impl Send for AutoSie {}
unsafe impl Sync for AutoSie {}

impl AutoSie {
    #[inline(always)]
    pub fn new() -> Self {
        local::always_local().sie_inc();
        Self
    }
}

impl Drop for AutoSie {
    fn drop(&mut self) {
        local::always_local().sie_dec();
    }
}

/// access user data and close interrupt.
pub struct AutoSum;

unsafe impl Send for AutoSum {}
unsafe impl Sync for AutoSum {}

impl AutoSum {
    pub fn new() -> Self {
        local::always_local().sum_inc();
        Self
    }
}

impl Drop for AutoSum {
    fn drop(&mut self) {
        let local = local::always_local();
        assert!(local.user_access_status.is_access());
        local.sum_dec();
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

pub async fn test() {
    let _auto_sum = AutoSum::new();
    stack_trace!();
    println!("[FTL OS]user_check test begin");
    let initproc = proc_table::get_initproc();
    let check = UserCheckImpl::new(&initproc);
    let mut array = 123u8;
    let rw_data = &mut array as *mut u8 as usize;
    let ro_data = "123456".as_ptr() as *const u8 as usize;
    let mut un_data = 1234567 as *const u8 as usize;
    check.read_check::<u8>(rw_data.into()).await.unwrap();
    check.read_check::<u8>(ro_data.into()).await.unwrap();
    check.read_check::<u8>(un_data.into()).await.unwrap_err();
    check.write_check::<u8>(rw_data.into()).await.unwrap();
    check.write_check::<u8>(ro_data.into()).await.unwrap_err();
    check.write_check::<u8>(un_data.into()).await.unwrap_err();

    use crate::memory::{address::UserAddr4K, map_segment::handler::map_all};
    let mut space = UserSpace::from_global().unwrap();
    let h = map_all::MapAllHandler::box_new(PTEFlags::U);
    let start = UserAddr4K::from_usize_check(0x1000);
    let range = start..start.add_one_page();
    space.map_segment.force_push(range, h).unwrap();
    unsafe { space.raw_using() };
    un_data = 0x1000 as *const u8 as usize;
    check.read_check::<u8>(un_data.into()).await.unwrap_err();
    check.write_check::<u8>(un_data.into()).await.unwrap_err();
    memory::set_satp_by_global();
    println!("[FTL OS]user_check test pass");
}
