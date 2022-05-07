mod sdcard;
mod virtio_blk;

pub use sdcard::SDCardWrapper;
pub use virtio_blk::VirtIOBlock;

use alloc::sync::Arc;

use crate::board::BlockDeviceImpl;

use super::BlockDevice;

static mut BLOCK_DEVICE: Option<Arc<dyn BlockDevice>> = None;

pub fn init() {
    println!("[FTL OS]driver init");
    // println!("[FTL OS]ignore dirver init");
    unsafe { BLOCK_DEVICE = Some(Arc::new(BlockDeviceImpl::new())) }
}

pub fn device() -> &'static Arc<dyn BlockDevice> {
    unsafe { BLOCK_DEVICE.as_ref().unwrap() }
}

#[allow(unused)]
pub fn block_device_test() {
    let x = core::lazy::Lazy::<usize>::new(|| 3);
    let block_device = device().clone();
    let mut write_buffer = [0u8; 512];
    let mut read_buffer = [0u8; 512];
    for i in 0..512 {
        for byte in write_buffer.iter_mut() {
            *byte = i as u8;
        }
        block_device.write_block(i as usize, &write_buffer);
        block_device.read_block(i as usize, &mut read_buffer);
        assert_eq!(write_buffer, read_buffer);
    }
    println!("block device test passed!");
}
