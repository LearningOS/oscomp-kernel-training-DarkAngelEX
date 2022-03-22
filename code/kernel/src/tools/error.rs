use core::fmt::Debug;

use crate::syscall::{SysError, UniqueSysError};

pub trait Error: Debug {}

pub trait OOM: Error {}

#[derive(Debug)]
pub struct FrameOOM;

impl From<FrameOOM> for SysError {
    fn from(_e: FrameOOM) -> Self {
        Self::ENOMEM
    }
}
impl From<FrameOOM> for UniqueSysError<{ SysError::ENOMEM as isize }> {
    fn from(_e: FrameOOM) -> Self {
        UniqueSysError
    }
}

impl Error for FrameOOM {}
impl OOM for FrameOOM {}

#[derive(Debug)]
pub struct HeapOOM;
impl From<()> for HeapOOM {
    fn from(_: ()) -> Self {
        Self
    }
}

impl Error for HeapOOM {}
impl OOM for HeapOOM {}

#[derive(Debug)]
pub struct TooManyUserStack;
impl Error for TooManyUserStack {}
