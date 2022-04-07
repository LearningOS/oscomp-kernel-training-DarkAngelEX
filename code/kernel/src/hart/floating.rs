use core::arch::asm;

use riscv::register::{
    fcsr::{RoundingMode, FCSR},
    sstatus::{self, Sstatus, FS},
};

use crate::trap::context::FloatContext;

pub const FLOAT_ENABLE: bool = true;
static mut FLOAT_FCSR: FCSR = unsafe { core::mem::transmute(0) };

pub fn default_fcsr() -> FCSR {
    unsafe { FLOAT_FCSR }
}

pub fn init() {
    println!("[FTL OS]float init");
    if !FLOAT_ENABLE {
        println!("float unavailable");
        return;
    }
    let round_mode = RoundingMode::RoundToNearestEven;
    // exception when NV(invalid operation)
    let fflags: u32 = 0b10000;
    let rm: u8 = unsafe { core::mem::transmute(round_mode) };
    let rm = rm as u32;
    let fr = rm << 4 | fflags;
    let xfr = unsafe { core::mem::transmute(fr) };
    unsafe {
        FLOAT_FCSR = xfr;
        stack_trace!();
        sstatus::set_fs(FS::Clean);
        asm!("csrw fcsr, {}", in(reg)fr);
    }
}

pub fn other_init() {
    if !FLOAT_ENABLE {
        return;
    }
    unsafe {
        sstatus::set_fs(FS::Clean);
        let fr: u32 = core::mem::transmute(FLOAT_FCSR);
        asm!("csrw fcsr, {}", in(reg)fr);
    }
}

/// memory -> register
#[target_feature(enable = "d")]
pub unsafe fn load_fx(fx: &mut FloatContext) {
    if fx.need_load == 0 {
        return;
    }
    fx.need_load = 0;
    asm!("
            fld  f0,  0*8({0})
            fld  f1,  1*8({0})
            fld  f2,  2*8({0})
            fld  f3,  3*8({0})
            fld  f4,  4*8({0})
            fld  f5,  5*8({0})
            fld  f6,  6*8({0})
            fld  f7,  7*8({0})
            fld  f8,  8*8({0})
            fld  f9,  9*8({0})
            fld f10, 10*8({0})
            fld f11, 11*8({0})
            fld f12, 12*8({0})
            fld f13, 13*8({0})
            fld f14, 14*8({0})
            fld f15, 15*8({0})
            fld f16, 16*8({0})
            fld f17, 17*8({0})
            fld f18, 18*8({0})
            fld f19, 19*8({0})
            fld f20, 20*8({0})
            fld f21, 21*8({0})
            fld f22, 22*8({0})
            fld f23, 23*8({0})
            fld f24, 24*8({0})
            fld f25, 25*8({0})
            fld f26, 26*8({0})
            fld f27, 27*8({0})
            fld f28, 28*8({0})
            fld f29, 29*8({0})
            fld f30, 30*8({0})
            fld f31, 31*8({0})
            lw  {0}, 32*8({0})
            csrw fcsr, {0}
        ", in(reg) fx
    );
}

pub fn store_fx_mark(fx: &mut FloatContext, ss: &mut Sstatus) {
    fx.need_save |= (ss.fs() == FS::Dirty) as u8;
}

pub fn switch_out(fx: &mut FloatContext) {
    fx.need_load = 1;
    unsafe { store_fx(fx) };
}

/// register -> memory
#[target_feature(enable = "d")]
unsafe fn store_fx(fx: &mut FloatContext) {
    if fx.need_save == 0 {
        return;
    }
    fx.need_save = 0;
    let mut t: usize = 1; // alloc a register but not zero.
    asm!("
            fsd  f0,  0*8({0})
            fsd  f1,  1*8({0})
            fsd  f2,  2*8({0})
            fsd  f3,  3*8({0})
            fsd  f4,  4*8({0})
            fsd  f5,  5*8({0})
            fsd  f6,  6*8({0})
            fsd  f7,  7*8({0})
            fsd  f8,  8*8({0})
            fsd  f9,  9*8({0})
            fsd f10, 10*8({0})
            fsd f11, 11*8({0})
            fsd f12, 12*8({0})
            fsd f13, 13*8({0})
            fsd f14, 14*8({0})
            fsd f15, 15*8({0})
            fsd f16, 16*8({0})
            fsd f17, 17*8({0})
            fsd f18, 18*8({0})
            fsd f19, 19*8({0})
            fsd f20, 20*8({0})
            fsd f21, 21*8({0})
            fsd f22, 22*8({0})
            fsd f23, 23*8({0})
            fsd f24, 24*8({0})
            fsd f25, 25*8({0})
            fsd f26, 26*8({0})
            fsd f27, 27*8({0})
            fsd f28, 28*8({0})
            fsd f29, 29*8({0})
            fsd f30, 30*8({0})
            fsd f31, 31*8({0})
            csrr {1}, fcsr
            sw  {1}, 32*8({0})
        ", in(reg) fx,
        inout(reg) t
    );
    drop(t);
}
