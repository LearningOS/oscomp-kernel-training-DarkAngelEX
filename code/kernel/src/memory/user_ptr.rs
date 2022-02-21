#![allow(dead_code)]
use core::marker::PhantomData;

use crate::config::USER_END;

pub trait Policy {}

pub trait Read: Policy {}
pub trait Write: Policy {}

pub struct In;
pub struct Out;
pub struct InOut;

impl Policy for In {}
impl Policy for Out {}
impl Policy for InOut {}
impl Read for In {}
impl Write for Out {}
impl Read for InOut {}
impl Write for InOut {}

pub struct UserPtr<T, P: Policy> {
    ptr: *mut T,
    _mark: PhantomData<P>,
}

pub type UserInPtr<T> = UserPtr<T, In>;
pub type UserOutPtr<T> = UserPtr<T, Out>;
pub type UserInOutPtr<T> = UserPtr<T, InOut>;

unsafe impl<T, P: Policy> Send for UserPtr<T, P> {}
unsafe impl<T, P: Policy> Sync for UserPtr<T, P> {}

impl<T, P: Policy> UserPtr<T, P> {
    pub fn from_usize(a: usize) -> Self {
        Self {
            ptr: a as *mut _,
            _mark: PhantomData,
        }
    }
    pub fn as_ptr(&self) -> Option<*const T> {
        if self.ptr == core::ptr::null_mut() || self.ptr as usize > USER_END {
            return None;
        }
        Some(self.ptr)
    }
}
impl<T, P: Read> UserPtr<T, P> {
    pub fn nonnull(&self) -> Option<*const T> {
        (self.ptr != core::ptr::null_mut()).then_some(self.ptr)
    }
}
impl<T, P: Write> UserPtr<T, P> {
    pub fn nonnull_mut(&self) -> Option<*mut T> {
        (self.ptr != core::ptr::null_mut()).then_some(self.ptr)
    }
}
