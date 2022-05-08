pub mod block;
mod hifive_spi;

pub use block::device;

pub use ftl_util::device::BlockDevice;

pub fn init() {
    println!("[FTL OS]driver init");
    block::init();
}

pub async fn test() {
    block::block_device_test().await;
}
