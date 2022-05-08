mod sdcard;
mod virtio_blk;

pub use sdcard::SDCardWrapper;
pub use virtio_blk::VirtIOBlock;

use alloc::sync::Arc;

use crate::board::BlockDeviceImpl;

use super::BlockDevice;

static mut BLOCK_DEVICE: Option<Arc<dyn BlockDevice>> = None;

pub fn init() {
    let device = match () {
        #[cfg(not(feature = "board_hifive"))]
        () => Arc::new(BlockDeviceImpl::new()),
        #[cfg(feature = "board_hifive")]
        () => super::hifive_spi::init_sdcard(),
    };
    unsafe { BLOCK_DEVICE = Some(device) }
}

pub fn device() -> &'static Arc<dyn BlockDevice> {
    unsafe { BLOCK_DEVICE.as_ref().unwrap() }
}

#[allow(unused)]
pub async fn block_device_test() {
    stack_trace!();
    println!("block device test begin");
    println!("block device test skip");
    let device = device();
    let mut buf0 = [0u8; 512];
    let mut buf1 = [0u8; 512];
    let mut buf2 = [0u8; 512];
    for i in 0..512 {
        for byte in buf0.iter_mut() {
            *byte = i as u8;
        }
        device.read_block(i as usize, &mut buf2).await.unwrap();
        device.write_block(i as usize, &buf0).await.unwrap();
        device.read_block(i as usize, &mut buf1).await.unwrap();
        device.write_block(i as usize, &buf2).await.unwrap();
        assert_eq!(buf0, buf1);
    }
    println!("block device test passed!");
}
