#![allow(clippy::upper_case_acronyms)]

use core::marker::PhantomData;

#[doc = "Universal register structure"]
#[repr(C)]
pub struct Reg<T: Sized + Clone + Copy, U> {
    value: T,
    p: PhantomData<U>,
}

impl<T: Sized + Clone + Copy, U> Reg<T, U> {
    pub fn new(initval: T) -> Self {
        Self {
            value: initval,
            p: PhantomData {},
        }
    }
}

impl<T: Sized + Clone + Copy, U> Reg<T, U> {
    pub fn read(&self) -> T {
        let ptr: *const T = &self.value;
        unsafe { ptr.read_volatile() }
    }
    pub fn write(&mut self, val: T) {
        let ptr: *mut T = &mut self.value;
        unsafe {
            ptr.write_volatile(val);
        }
    }
}

pub struct _RESERVED;
pub type RESERVED = Reg<u32, _RESERVED>;

pub struct _SCKDIV;
pub type SCKDIV = Reg<u32, _SCKDIV>;
impl SCKDIV {
    pub fn reset(&mut self) {
        self.write(3u32);
    }
}

pub struct _SCKMODE;
pub type SCKMODE = Reg<u32, _SCKMODE>;
impl SCKMODE {
    pub fn reset(&mut self) {
        self.write(0u32);
    }
    pub fn set_phase(&mut self, phasebit: bool) {
        let mut data = self.read();
        data = if phasebit {
            data | 0x1u32
        } else {
            data & 0xfffeu32
        };
        self.write(data);
    }
    pub fn set_polarity(&mut self, polbit: bool) {
        let mut data = self.read();
        data = if polbit {
            data | 0x2u32
        } else {
            data & 0xfffdu32
        };
        self.write(data);
    }
}

pub struct _CSID;
pub type CSID = Reg<u32, _CSID>;
impl CSID {
    pub fn reset(&mut self) {
        self.write(0u32);
    }
}

pub struct _CSDEF;
pub type CSDEF = Reg<u32, _CSDEF>;
impl CSDEF {
    pub fn reset(&mut self) {
        self.write(1u32);
    }
    pub fn cs_active_low(&mut self) {
        self.write(1u32);
    }
    pub fn cs_active_high(&mut self) {
        self.write(0u32);
    }
}

pub struct _CSMODE;
pub type CSMODE = Reg<u32, _CSMODE>;
impl CSMODE {
    pub fn reset(&mut self) {
        self.write(0u32);
    }
}
#[derive(Copy, Clone)]
pub enum Mode {
    AUTO,
    HOLD,
    OFF,
}
impl CSMODE {
    pub fn switch_csmode(&mut self, mode: Mode) {
        self.write(match mode {
            Mode::AUTO => 0u32,
            Mode::HOLD => 2u32,
            Mode::OFF => 3u32,
        });
    }
}

pub struct _DELAY0;
pub type DELAY0 = Reg<u32, _DELAY0>;
impl DELAY0 {
    pub fn reset(&mut self) {
        self.write(0x00010001u32);
    }
    pub fn get_cssck(&self) -> u8 {
        let data = self.read();
        data as u8
    }
    pub fn set_cssck(&mut self, value: u8) {
        let mut data = self.read();
        data = (data & 0xffff0000u32) | value as u32;
        self.write(data);
    }

    pub fn get_sckcs(&self) -> u8 {
        let data = self.read();
        (data >> 16) as u8
    }
    pub fn set_sckcs(&mut self, value: u8) {
        let mut data = self.read();
        data = (data & 0x0000ffffu32) | ((value as u32) << 16);
        self.write(data);
    }
}

pub struct _DELAY1;
pub type DELAY1 = Reg<u32, _DELAY1>;
impl DELAY1 {
    pub fn reset(&mut self) {
        self.write(0x00000001u32);
    }
    pub fn get_intercs(&self) -> u8 {
        let data = self.read();
        data as u8
    }
    pub fn set_intercs(&mut self, value: u8) {
        let mut data = self.read();
        data = (data & 0xffff0000u32) | value as u32;
        self.write(data);
    }

    pub fn get_interxfr(&self) -> u8 {
        let data = self.read();
        (data >> 16) as u8
    }
    pub fn set_interxfr(&mut self, value: u8) {
        let mut data = self.read();
        data = (data & 0x0000ffffu32) | ((value as u32) << 16);
        self.write(data);
    }
}

pub struct _FMT;
pub type FMT = Reg<u32, _FMT>;
#[derive(Copy, Clone)]
pub enum Protocol {
    Single,
    Dual,
    Quad,
}

impl FMT {
    pub fn reset(&mut self) {
        self.write(0x00080000u32);
    }
    pub fn switch_protocol(&mut self, proto: Protocol) {
        let p = match proto {
            Protocol::Single => 0u32,
            Protocol::Dual => 1u32,
            Protocol::Quad => 2u32,
        };
        let r = self.read();
        self.write((r & (!0b011u32)) | p);
    }

    // TODO FIX BITWISE OPS
    pub fn set_endian(&mut self, msb: bool) {
        let end = if !msb { 0b100u32 } else { 0u32 };
        let r = self.read();
        self.write((r & (!0b100u32)) | end);
    }

    /// true: 发送 false: 接收
    ///
    /// 设置为true时不再响应 receive FIFO
    pub fn set_direction(&mut self, send: bool) {
        let dir = if send { 0b1000u32 } else { 0u32 };
        let r = self.read();
        self.write((r & (!0b1000u32)) | dir);
    }

    pub fn set_len(&mut self, frame_size: u8) {
        let fs = (frame_size as u32 & 0x0fu32) << 16;
        let mask = 0xfu32 << 16;
        let r = self.read();
        self.write((r & !mask) | fs);
    }
}

pub struct _TXDATA;
pub type TXDATA = Reg<u32, _TXDATA>;
impl TXDATA {
    pub fn is_full(&self) -> bool {
        let r = self.read();
        (r & (1u32 << 31)) != 0u32
    }
}

pub struct _RXDATA;
pub type RXDATA = Reg<u32, _RXDATA>;
impl RXDATA {
    pub fn is_empty(&self) -> bool {
        let r = self.read();
        (r & (1u32 << 31)) != 0u32
    }
    /// return (is_empty, result)
    pub fn flag_read(&self) -> (bool, u8) {
        let r = self.read();
        let is_empty = (r & (1u32 << 31)) != 0u32;
        (is_empty, r as u8)
    }
}

pub struct _TXMARK;
pub type TXMARK = Reg<u32, _TXMARK>;
impl TXMARK {
    pub fn reset(&mut self) {
        self.write(0u32);
    }
    /// 实际写入值为n-1
    pub fn set_wait_num(&mut self, n: usize) {
        debug_assert!(n != 0 && n <= 8);
        self.write((9 - n).min(7) as u32);
    }
}

pub struct _RXMARK;
pub type RXMARK = Reg<u32, _RXMARK>;
impl RXMARK {
    pub fn reset(&mut self) {
        self.write(0u32);
    }
    /// 实际写入值为n-1
    pub fn set_wait_num(&mut self, n: usize) {
        debug_assert!(n != 0 && n <= 8);
        self.write((n - 1) as u32);
    }
}

pub struct _FCTRL;
pub type FCTRL = Reg<u32, _FCTRL>;
impl FCTRL {
    pub fn reset(&mut self) {
        self.write(1u32);
    }
    pub fn set_flash_mode(&mut self, mmio_enable: bool) {
        let v = if mmio_enable { 1u32 } else { 0u32 };
        self.write(v);
    }
}

pub struct _FFMT;
pub type FFMT = Reg<u32, _FFMT>;
impl FFMT {
    pub fn reset(&mut self) {
        self.write(0x00030007);
    }
    fn set_cmden(&mut self, en: bool) {
        let v = if en { 1u32 } else { 0u32 };
        let mask = 1u32;
        let r = self.read();
        self.write((r & !mask) | v);
    }

    fn set_addrlen(&mut self, len: u8) {
        let v = (len as u32 & 0x7u32) << 1;
        let mask = 0xeu32;
        let r = self.read();
        self.write((r & !mask) | v);
    }

    fn set_padcnt(&mut self, padcnt: u8) {
        let v = (padcnt as u32 & 0xfu32) << 4;
        let mask = 0xf0u32;
        let r = self.read();
        self.write((r & !mask) | v);
    }

    fn set_cmdproto(&mut self, proto: u8) {
        let v = (proto as u32 & 0x3u32) << 8;
        let mask = 0x300u32;
        let r = self.read();
        self.write((r & !mask) | v);
    }

    fn set_addrproto(&mut self, proto: u8) {
        let v = (proto as u32 & 0x3u32) << 10;
        let mask = 0xc00u32;
        let r = self.read();
        self.write((r & !mask) | v);
    }

    fn set_dataproto(&mut self, proto: u8) {
        let v = (proto as u32 & 0x3u32) << 12;
        let mask = 0x3000u32;
        let r = self.read();
        self.write((r & !mask) | v);
    }

    fn set_cmdcode(&mut self, code: u8) {
        let v = (code as u32) << 16;
        let mask = 0xfu32 << 16;
        let r = self.read();
        self.write((r & !mask) | v);
    }

    fn set_padcode(&mut self, code: u8) {
        let v = (code as u32) << 24;
        let mask = 0xfu32 << 24;
        let r = self.read();
        self.write((r & !mask) | v);
    }
}

pub struct _IE;
pub type IE = Reg<u32, _IE>;
impl IE {
    pub fn reset(&mut self) {
        self.write(0u32);
    }
    pub fn set_transmit_watermark(&mut self, enable: bool) {
        let en = if enable { 1u32 } else { 0u32 };
        let r = self.read();
        self.write((r & (!1u32)) | en);
    }

    pub fn set_receive_watermark(&mut self, enable: bool) {
        let en = if enable { 2u32 } else { 0u32 };
        let r = self.read();
        self.write((r & (!2u32)) | en);
    }
}

pub struct _IP;
pub type IP = Reg<u32, _IP>;
impl IP {
    pub fn transmit_pending(&self) -> bool {
        let r = self.read();
        (r & 1u32) != 0
    }

    pub fn receive_pending(&self) -> bool {
        let r = self.read();
        (r & 2u32) != 0
    }
}
