use core::{future::Future, pin::Pin};

use alloc::boxed::Box;

pub type Async<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub type AsyncRet<'a> = Async<'a, Result<(), ()>>;

/// 此驱动参数为逻辑扇区，文件系统的第一个扇区为0
pub trait LogicBlockDevice: Send + Sync + 'static {
    fn read_block<'a>(&'a self, block_id: usize, buf: &'a mut [u8]) -> AsyncRet<'a>;
    fn write_block(&self, block_id: usize, buf: &[u8]) -> AsyncRet;
}
