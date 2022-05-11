mod sdcard;
mod virtio_blk;

pub use sdcard::SDCardWrapper;
pub use virtio_blk::VirtIOBlock;

pub const BPB_CID: usize = 10274;

use alloc::sync::Arc;

use super::BlockDevice;

static mut BLOCK_DEVICE: Option<Arc<dyn BlockDevice>> = None;

pub fn init() {
    stack_trace!();
    let device = match () {
        #[cfg(not(feature = "board_hifive"))]
        () => Arc::new(BlockDeviceImpl::new()),
        #[cfg(feature = "board_hifive")]
        () => {
            Arc::new(super::spi_sd::SDCardWrapper::new())
            // super::blockdev::init_sdcard()
        }
    };
    unsafe { BLOCK_DEVICE = Some(device) }
}

pub fn device() -> &'static Arc<dyn BlockDevice> {
    unsafe { BLOCK_DEVICE.as_ref().unwrap() }
}

#[allow(unused)]
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
    device.read_block(0, &mut buf2).await.unwrap();
    println!("0: {:?}", buf2);

    buf0.fill(123);
    device.write_block(10000, &buf0).await.unwrap();
    device.read_block(10000, &mut buf1).await.unwrap();
    println!("10000: {:?}", buf1);

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
