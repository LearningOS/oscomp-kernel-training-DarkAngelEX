mod spi_sd;

use alloc::sync::Arc;
use fat32::BlockDevice;

use spi_sd::SDCardWrapper;

#[allow(dead_code)]
pub fn init_sdcard() -> Arc<dyn BlockDevice> {
    stack_trace!();
    Arc::new(SDCardWrapper::new())
}
