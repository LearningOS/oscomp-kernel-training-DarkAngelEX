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

pub struct _UNUSEDNOW;
pub type UNUSEDNOW = Reg<u32, _UNUSEDNOW>;

pub struct _CorePllCfg;
pub type CorePllCfg = Reg<u32, _CorePllCfg>;
impl CorePllCfg {
    pub fn pll_lock(&self) -> bool {
        self.read() & (1 << 31) != 0
    }
    pub fn wait_lock(&self) {
        while !self.pll_lock() {}
    }
    pub fn set_1500mhz(&mut self) {
        self.set_rfq(0, 57, 1);
    }
    pub fn set_rfq(&mut self, pllr: u32, pllf: u32, pllq: u32) {
        let mut v = self.read();
        v &= !((1 << 18) - 1);
        v |= pllr;
        v |= pllf << 6;
        v |= pllq << 15;
        self.write(v);
    }
}

pub struct _CoreClkSelReg;
pub type CoreClkSelReg = Reg<u32, _CoreClkSelReg>;
impl CoreClkSelReg {
    pub fn using_coreclk(&mut self) {
        self.write(0);
    }
    pub fn using_hfclk(&mut self) {
        self.write(1);
    }
}
