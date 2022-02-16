use core::fmt::Debug;

pub trait Error: Debug {}

pub trait OutOfMemory: Error {}

#[derive(Debug)]
pub struct FrameOutOfMemory;

impl Error for FrameOutOfMemory {}
impl OutOfMemory for FrameOutOfMemory {}

#[derive(Debug)]
pub struct HeapOutOfMemory;

impl Error for HeapOutOfMemory {}
impl OutOfMemory for HeapOutOfMemory {}

#[derive(Debug)]
pub struct TooManyUserStack;
impl Error for TooManyUserStack {}