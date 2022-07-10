use riscv::register::fcsr::FCSR;

use crate::{hart::floating, memory::user_ptr::UserInOutPtr, trap::context::UKContext};

use super::{SignalSet, SignalStack};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SignalContext {
    pub flags: usize,
    pub scx_ptr: UserInOutPtr<SignalContext>,
    pub stack: SignalStack,
    pub mask: SignalSet,
    pub urx: [usize; 32],
    pub ufx: [f64; 32],
    pub fcsr: FCSR,
    pub sepc: usize,
}

impl SignalContext {
    pub fn load(
        &mut self,
        uk_cx: &mut UKContext,
        scx_ptr: UserInOutPtr<SignalContext>,
        mask: SignalSet,
    ) {
        self.scx_ptr = scx_ptr;
        // ignore stack
        self.mask = mask;
        self.urx = uk_cx.user_rx;
        unsafe { floating::load_fx(&mut uk_cx.user_fx) };
        self.ufx = uk_cx.user_fx.fx;
        self.fcsr = uk_cx.user_fx.fcsr;
        self.sepc = uk_cx.user_sepc;
    }
    pub fn store(&self, uk_cx: &mut UKContext) -> (UserInOutPtr<SignalContext>, &SignalSet) {
        // ignore stack
        uk_cx.user_rx = self.urx;
        if uk_cx.user_fx.sig_dirty != 0 {
            uk_cx.user_fx.fx = self.ufx;
            uk_cx.user_fx.sig_dirty = 0;
        }
        uk_cx.user_fx.fcsr = self.fcsr;
        uk_cx.user_sepc = self.sepc;
        (self.scx_ptr, &self.mask)
    }
}
