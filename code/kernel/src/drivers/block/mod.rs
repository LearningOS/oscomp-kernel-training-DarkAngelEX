use ftl_util::async_tools::ASysR;

#[cfg(feature = "board_k210")]
pub use sdcard::SDCardWrapper;

pub use virtio_blk::VirtIOBlock;

#[cfg(not(feature = "submit"))]
pub const BPB_CID: usize = 10274;
#[cfg(feature = "submit")]
pub const BPB_CID: usize = 0;

use alloc::{boxed::Box, sync::Arc};

use crate::config::KERNEL_OFFSET_FROM_DIRECT_MAP;

use super::BlockDevice;

#[cfg(feature = "board_k210")]
mod sdcard;
mod virtio_blk;

static mut BLOCK_DEVICE: Option<Arc<dyn BlockDevice>> = None;

pub fn init() {
    stack_trace!();
    let device: Arc<dyn BlockDevice> = match () {
        #[cfg(not(feature = "board_hifive"))]
        () => Arc::new(crate::board::BlockDeviceImpl::new()),
        #[cfg(feature = "board_hifive")]
        () => {
            // Arc::new(super::spi_sd::SDCardWrapper::new()) // hifive
            // super::blockdev::init_sdcard() // k210
            Arc::new(MemDriver) // 0x9000_0000
        }
    };
    unsafe { BLOCK_DEVICE = Some(device) }
}

pub fn device() -> &'static Arc<dyn BlockDevice> {
    unsafe { BLOCK_DEVICE.as_ref().unwrap() }
}

#[allow(unused)]
#[inline(never)]
pub async fn block_device_test() {
    stack_trace!();
    if cfg!(not(feature = "board_hifive")) {
        println!("block device test skip");
        return;
    }

    let test_cnt = 3;

    println!("block device test begin");
    let device = device().as_ref();
    let mut buf0 = [0u8; 512];
    let mut buf1 = [0u8; 512];
    let mut buf2 = [0u8; 512];
    if false {
        device.read_block(0, &mut buf2).await.unwrap();
        println!("0: {:?}", buf2);
    }
    for i in 1..test_cnt {
        for (j, byte) in buf0.iter_mut().enumerate() {
            *byte = (i + j) as u8;
        }
        let bid = i + 10000;
        device.read_block(bid, &mut buf2).await.unwrap();
        device.write_block(bid, &buf0).await.unwrap();
        device.read_block(bid, &mut buf1).await.unwrap();
        device.write_block(bid, &buf2).await.unwrap();
        assert_eq!(buf0, buf1);
    }
    println!("block device test passed!");
}

struct MemDriver;

const BASE_ADDR: usize = 0x9000_0000 + KERNEL_OFFSET_FROM_DIRECT_MAP;

impl MemDriver {
    fn block_range(block_id: usize, len: usize) -> &'static mut [u8] {
        let start = BASE_ADDR + block_id * 512;
        unsafe { core::slice::from_raw_parts_mut(start as *mut u8, len) }
    }
}

impl BlockDevice for MemDriver {
    fn sector_bpb(&self) -> usize {
        0
    }
    fn sector_bytes(&self) -> usize {
        512
    }
    fn read_block<'a>(&'a self, block_id: usize, buf: &'a mut [u8]) -> ASysR<'a, ()> {
        Box::pin(async move {
            buf.copy_from_slice(Self::block_range(block_id, buf.len()));
            Ok(())
        })
    }
    fn write_block<'a>(&'a self, block_id: usize, buf: &'a [u8]) -> ASysR<'a, ()> {
        Box::pin(async move {
            Self::block_range(block_id, buf.len()).copy_from_slice(buf);
            Ok(())
        })
    }
}
