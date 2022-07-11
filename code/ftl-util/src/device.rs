use crate::async_tools::AsyncRet;

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
