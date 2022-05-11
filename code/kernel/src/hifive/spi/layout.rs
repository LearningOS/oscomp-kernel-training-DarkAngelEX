use super::registers::*;
use core::ops::{Deref, DerefMut};

use crate::{hifive::clock::HF_CLK, memory::address::PhyAddr};

use super::SPIActions;

/** SPI registers encapsulation */

#[derive(Copy, Clone)]
pub enum SPIDevice {
    QSPI0,
    QSPI1,
    QSPI2,
    Other(usize),
}

impl Deref for SPIDevice {
    type Target = RegisterBlock;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.base_addr().into_ref().get() }
    }
}

impl DerefMut for SPIDevice {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.base_addr().into_ref().get_mut() }
    }
}

impl SPIDevice {
    fn base_addr(&self) -> PhyAddr<RegisterBlock> {
        let a = match self {
            SPIDevice::QSPI0 => 0x10040000usize,
            SPIDevice::QSPI1 => 0x10041000usize,
            SPIDevice::QSPI2 => 0x10050000usize,
            SPIDevice::Other(val) => val.clone(),
        };
        PhyAddr::from_usize(a)
    }
}

#[doc = "For more information, see: https://sifive.cdn.prismic.io/sifive/d3ed5cd0-6e74-46b2-a12d-72b06706513e_fu540-c000-manual-v1p4.pdf"]
#[repr(C)]
pub struct RegisterBlock {
    #[doc = "0x00: Serial clock divisor register"]
    pub sckdiv: SCKDIV,
    #[doc = "0x04: Serial clock mode register"]
    pub sckmode: SCKMODE,
    #[doc = "0x08: Reserved"]
    _reserved0: RESERVED,
    #[doc = "0x0c: Reserved"]
    _reserved1: RESERVED,
    #[doc = "0x10: Chip select ID register"]
    pub csid: CSID,
    #[doc = "0x14: Chip select default register"]
    pub csdef: CSDEF,
    #[doc = "0x18: Chip Select mode register"]
    pub csmode: CSMODE,
    #[doc = "0x1C: Reserved"]
    _reserved2: RESERVED,
    #[doc = "0x20: Reserved"]
    _reserved3: RESERVED,
    #[doc = "0x24: Reserved"]
    _reserved4: RESERVED,
    #[doc = "0x28: Delay control 0 register"]
    pub delay0: DELAY0,
    #[doc = "0x2C: Delay control 1 register"]
    pub delay1: DELAY1,
    #[doc = "0x30: Reserved"]
    _reserved5: RESERVED,
    #[doc = "0x34: Reserved"]
    _reserved6: RESERVED,
    #[doc = "0x38: Reserved"]
    _reserved7: RESERVED,
    #[doc = "0x3C: Reserved"]
    _reserved8: RESERVED,
    #[doc = "0x40: Frame format register ()"]
    pub fmt: FMT,
    #[doc = "0x44: Reserved"]
    _reserved9: RESERVED,
    #[doc = "0x48: Tx FIFO data register"]
    pub txdata: TXDATA,
    #[doc = "0x4C: Rx FIFO data register"]
    pub rxdata: RXDATA,
    #[doc = "0x50: Tx FIFO watermark"]
    pub txmark: TXMARK,
    #[doc = "0x54: Rx FIFO watermark"]
    pub rxmark: RXMARK,
    #[doc = "0x58: Reserved"]
    _reserved10: RESERVED,
    #[doc = "0x5C: Reserved"]
    _reserved11: RESERVED,
    #[doc = "0x60: SPI flash interface control register"]
    pub fctrl: FCTRL,
    #[doc = "0x64: SPI flash instruction format register"]
    pub ffmt: FFMT,
    #[doc = "0x68: Reserved"]
    _reserved12: RESERVED,
    #[doc = "0x6C: Reserved"]
    _reserved13: RESERVED,
    #[doc = "0x70: SPI flash interrupt enable register"]
    pub ie: IE,
    #[doc = "0x74: SPI flash interrupt pending register"]
    pub ip: IP,
}

pub struct SPIImpl {
    spi: SPIDevice,
}

/** SPI abstraction implementation */

impl SPIImpl {
    pub fn new(spi: SPIDevice) -> Self {
        Self { spi }
    }
}

impl SPIImpl {
    fn tx_fill(&mut self, data: u8, mut n: usize) {
        while n != 0 && !self.spi.txdata.is_full() {
            self.spi.txdata.write(data as u32);
            n -= 1;
        }
    }
    fn tx_enque(&mut self, data: u8) {
        debug_assert!(!self.spi.txdata.is_full());
        self.spi.txdata.write(data as u32);
    }
    fn rx_deque(&mut self) -> u8 {
        match self.spi.rxdata.flag_read() {
            (false, result) => return result,
            (true, _) => panic!(),
        }
    }
    // 返回可以取出的最大数据数量
    fn rx_wait(&self) {
        while !self.spi.ip.receive_pending() {
            // loop
        }
    }
    // 返回可以发送的最大数据数量
    fn tx_wait(&self) {
        while !self.spi.ip.transmit_pending() {
            // loop
        }
    }
}

impl SPIActions for SPIImpl {
    // This function references spi-sifive.c:sifive_spi_init()
    fn init(&mut self) {
        stack_trace!();
        println!("SPIImpl init start");
        let spi = &mut *self.spi;

        let inactive = spi.csdef.read();
        spi.csdef.write(u32::MAX);
        let cs_bit = spi.csdef.read();
        spi.csdef.write(inactive);
        println!("### cs_bit: {:#x} ###", cs_bit);

        println!("SPIImpl init 0");
        //  Watermark interrupts are disabled by default
        spi.ie.set_transmit_watermark(false);
        println!("SPIImpl init 1");
        spi.ie.set_receive_watermark(false);

        // Default watermark FIFO threshold values
        spi.txmark.write(1u32);
        spi.rxmark.write(0u32);

        // Set CS/SCK Delays and Inactive Time to defaults
        spi.delay0.set_cssck(1);
        spi.delay0.set_sckcs(1);
        spi.delay1.set_intercs(1);
        spi.delay1.set_interxfr(0);

        // Exit specialized memory-mapped SPI flash mode
        spi.fctrl.set_flash_mode(false);
        println!("SPIImpl init end");
    }

    fn configure(&mut self, use_lines: u8, data_bit_length: u8, msb_first: bool) {
        let spi = &mut *self.spi;
        // bit per word
        spi.fmt.set_len(data_bit_length);
        // switch protocol (QSPI, DSPI, SPI)
        let fmt_proto = match use_lines {
            4u8 => Protocol::Quad,
            2u8 => Protocol::Dual,
            _ => Protocol::Single,
        };
        spi.fmt.switch_protocol(fmt_proto);
        // endianness
        spi.fmt.set_endian(msb_first);
        // clock mode: from sifive_spi_prepare_message
        spi.sckmode.reset();
    }

    fn switch_cs(&mut self, enable: bool, csid: u32) {
        // manual cs
        self.spi
            .csmode
            .switch_csmode(if enable { Mode::HOLD } else { Mode::OFF });
        self.spi.csid.write(csid);
    }

    /// f_out = f_in / (2 * (div + 1))
    ///
    /// when div = 3000, f_out = 4kHz
    fn set_clk_rate(&mut self, spi_clk: usize) {
        // calculate clock rate
        let div = HF_CLK / (2 * spi_clk) - 1;
        debug_assert!(div < 1 << 12); //
        self.spi.sckdiv.write(div as u32);
    }

    fn recv_data(&mut self, chip_select: u32, rx_buf: &mut [u8]) {
        stack_trace!();
        // direction
        self.spi.fmt.set_direction(false);
        // select the correct device
        self.spi.csid.write(chip_select);

        for s in rx_buf.chunks_mut(8) {
            let n = s.len();
            self.tx_fill(0xff, n);
            // self.spi.txmark.set_wait_num(n);
            // self.tx_wait();
            // (0..n).for_each(|_| self.tx_enque(0xff));
            self.spi.rxmark.set_wait_num(n);
            self.rx_wait();
            s.fill_with(|| self.rx_deque());
        }
    }
    // 使用此函数之前需要设置fmt.dir为1
    fn send_data(&mut self, chip_select: u32, tx_buf: &[u8]) {
        stack_trace!();
        // direction
        self.spi.fmt.set_direction(true);
        // select the correct device
        self.spi.csid.write(chip_select);

        for s in tx_buf.chunks(8) {
            let n = s.len();
            self.spi.txmark.set_wait_num(n);
            self.tx_wait();
            s.iter().for_each(|&x| self.tx_enque(x));
        }
    }
}

/* testings */

/*
#[test]
fn reg_blk_test() {
  use registers::*;
  let a = [0u32; 0x50];
  let b = &a as *const _ as usize as *const RegisterBlock;
  let c: &RegisterBlock = unsafe { &*b };
  c.fmt.write(0x1234u32);
  assert_eq!(0x1024u32, c.fmt.read());
}
*/
