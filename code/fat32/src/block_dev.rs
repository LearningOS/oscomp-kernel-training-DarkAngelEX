use core::{future::Future, pin::Pin};

use alloc::boxed::Box;
use ftl_util::error::SysError;

pub type Async<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub type AsyncRet<'a> = Async<'a, Result<(), SysError>>;

/// buf的长度必须为sector_bytes的倍数
pub trait BlockDevice: Send + Sync + 'static {
    /// 此分区所在的第一个扇区号
    fn sector_bpb(&self) -> usize;
    /// 扇区大小 一定是2的幂次
    fn sector_bytes(&self) -> usize;
    /// device -> buf
    #[must_use]
    fn read_block<'a>(&'a self, block_id: usize, buf: &'a mut [u8]) -> AsyncRet<'a>;
    /// buf -> device
    #[must_use]
    fn write_block<'a>(&'a self, block_id: usize, buf: &'a [u8]) -> AsyncRet<'a>;
}

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
