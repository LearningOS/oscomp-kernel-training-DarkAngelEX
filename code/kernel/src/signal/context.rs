use riscv::register::fcsr::FCSR;

use crate::{hart::floating, memory::user_ptr::UserInOutPtr, trap::context::UKContext};

use super::{SignalSet, SignalSetDummy, SignalStack};

#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct Align16;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SignalContext {
    pub flags: usize,
    pub scx_ptr: UserInOutPtr<SignalContext>,
    pub stack: SignalStack,
    pub mask: SignalSet,
    pub dummy: SignalSetDummy,
    align16: Align16,
    pub urx: [usize; 32],
    pub ufx: [f64; 32],
    pub fcsr: FCSR,
}

impl SignalContext {
    fn set_sepc(&mut self, sepc: usize) {
        self.urx[0] = sepc;
    }
    fn sepc(&self) -> usize {
        self.urx[0]
    }
    pub fn load(
        &mut self,
        uk_cx: &mut UKContext,
        scx_ptr: UserInOutPtr<SignalContext>,
        mask: SignalSet,
    ) {
        self.scx_ptr = scx_ptr;
        // ignore stack
        self.mask = mask;
        self.set_sepc(uk_cx.user_sepc);
        self.urx[1..].copy_from_slice(&uk_cx.user_rx[1..]);
        unsafe { floating::load_fx(&mut uk_cx.user_fx) };
        self.ufx = uk_cx.user_fx.fx;
        self.fcsr = uk_cx.user_fx.fcsr;
    }
    pub fn store(&self, uk_cx: &mut UKContext) -> (UserInOutPtr<SignalContext>, &SignalSet) {
        // ignore stack
        uk_cx.user_rx[1..].copy_from_slice(&self.urx[1..]);
        if uk_cx.user_fx.sig_dirty != 0 {
            uk_cx.user_fx.fx = self.ufx;
            uk_cx.user_fx.sig_dirty = 0;
        }
        uk_cx.user_fx.fcsr = self.fcsr;
        uk_cx.user_sepc = self.sepc();
        (self.scx_ptr, &self.mask)
    }
}
