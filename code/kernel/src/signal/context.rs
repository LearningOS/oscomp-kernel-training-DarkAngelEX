use crate::{memory::user_ptr::UserInOutPtr, trap::context::UKContext};

use super::SignalSet;

#[derive(Clone, Copy)]
pub struct SignalContext {
    pub urx: [usize; 32],
    pub ufx: [f64; 32],
    pub sepc: usize,
    pub scx_ptr: UserInOutPtr<SignalContext>,
    pub mask: SignalSet,
}

impl SignalContext {
    pub fn load(
        &mut self,
        uk_cx: &UKContext,
        scx_ptr: UserInOutPtr<SignalContext>,
        mask: SignalSet,
    ) {
        self.urx = uk_cx.user_rx;
        self.ufx = uk_cx.user_fx.fx;
        self.sepc = uk_cx.user_sepc;
        self.scx_ptr = scx_ptr;
        self.mask = mask;
    }
}
