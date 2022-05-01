use core::{future::Future, pin::Pin};

use alloc::boxed::Box;
use ftl_util::{device::BlockDevice, error::SysError};

pub type Async<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub type AsyncRet<'a> = Async<'a, Result<(), SysError>>;

/// 一个占位用的初始化BlockDevice 避免写 Option<Arc<>>
pub struct PanicBlockDevice;

impl BlockDevice for PanicBlockDevice {
    fn sector_bpb(&self) -> usize {
        panic!()
    }
    fn sector_bytes(&self) -> usize {
        panic!()
    }
    fn read_block<'a>(&'a self, _block_id: usize, _buf: &'a mut [u8]) -> AsyncRet<'a> {
        panic!()
    }
    fn write_block<'a>(&'a self, _block_id: usize, _buf: &'a [u8]) -> AsyncRet<'a> {
        panic!()
    }
}
