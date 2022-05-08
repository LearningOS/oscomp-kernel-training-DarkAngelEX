pub mod spi_sd;

use alloc::sync::Arc;
use ftl_util::device::BlockDevice;

use spi_sd::SDCardWrapper;

#[allow(dead_code)]
pub fn init_sdcard() -> Arc<dyn BlockDevice> {
    Arc::new(SDCardWrapper::new())
}
