use ftl_util::{async_tools::ASysR, device::BlockDevice};

/// 一个占位用的初始化BlockDevice 避免写 Option<Arc<>>
pub struct PanicBlockDevice;

impl BlockDevice for PanicBlockDevice {
    fn sector_bpb(&self) -> usize {
        panic!()
    }
    fn sector_bytes(&self) -> usize {
        panic!()
    }
    fn read_block<'a>(&'a self, _block_id: usize, _buf: &'a mut [u8]) -> ASysR<()> {
        panic!()
    }
    fn write_block<'a>(&'a self, _block_id: usize, _buf: &'a [u8]) -> ASysR<()> {
        panic!()
    }
}
