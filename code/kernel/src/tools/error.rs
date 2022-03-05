use core::fmt::Debug;

use crate::syscall::{SysError, UniqueSysError};

pub trait Error: Debug {}

pub trait OutOfMemory: Error {}

#[derive(Debug)]
pub struct FrameOutOfMemory;

impl From<FrameOutOfMemory> for SysError {
    fn from(_e: FrameOutOfMemory) -> Self {
        Self::ENOMEM
    }
}
impl From<FrameOutOfMemory> for UniqueSysError<{ SysError::ENOMEM as isize }> {
    fn from(_e: FrameOutOfMemory) -> Self {
        UniqueSysError
    }
}

impl Error for FrameOutOfMemory {}
impl OutOfMemory for FrameOutOfMemory {}

#[derive(Debug)]
pub struct HeapOutOfMemory;

impl Error for HeapOutOfMemory {}
impl OutOfMemory for HeapOutOfMemory {}

#[derive(Debug)]
pub struct TooManyUserStack;
impl Error for TooManyUserStack {}
