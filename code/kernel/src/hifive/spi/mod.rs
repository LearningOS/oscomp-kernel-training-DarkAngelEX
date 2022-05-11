mod layout;
pub mod registers;

pub use layout::{SPIDevice, SPIImpl};

pub trait SPIActions {
    fn init(&mut self);
    fn configure(
        &mut self,
        use_lines: u8,       // SPI data line width, 1,2,4 allowed
        data_bit_length: u8, // bits per word, basically 8
        msb_first: bool,     // endianness
    );
    fn switch_cs(&mut self, enable: bool, csid: u32);
    fn set_clk_rate(&mut self, spi_clk: usize);
    fn recv_data(&mut self, chip_select: u32, rx: &mut [u8]);
    fn send_data(&mut self, chip_select: u32, tx: &[u8]);
}
