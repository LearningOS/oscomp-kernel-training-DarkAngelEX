pub mod spi_sd;

use alloc::sync::Arc;
use ftl_util::device::BlockDevice;

pub fn init_sdcard() -> Arc<dyn BlockDevice> {
    unsafe {
        // BLOCK_DEVICE = Some(Arc::new(spi_sd::SDCardWrapper::new()));
        // BLOCK_DEVICE.as_ref().unwrap().init();
        todo!()
    }
}
