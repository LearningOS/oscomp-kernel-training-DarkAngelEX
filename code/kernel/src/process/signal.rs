use crate::signal::{Action, SigAction, StdSignalSet};

pub struct ProcSignalManager {
    pending: StdSignalSet, // 等待处理的信号
    action: [SigAction; 32],
}

impl ProcSignalManager {
    pub fn new() -> Self {
        Self {
            pending: StdSignalSet::empty(),
            action: SigAction::DEFAULT_SET,
        }
    }
    pub fn send(&mut self, signal_set: StdSignalSet) {
        self.pending.insert(signal_set)
    }
    // 找到数字最小的信号并处理，返回处理方式和旧的阻塞集
    pub fn take_signal(&mut self, mask: StdSignalSet) -> Option<(Action, StdSignalSet)> {
        stack_trace!();
        if self.pending.contain_never_capture() {
            return Some((Action::Abort, mask));
        }
        let can_slot = self.pending.difference(mask);
        if can_slot.is_empty() {
            return None;
        }
        let idx = can_slot.bits().leading_zeros();
        let action = self.action[idx as usize].get_action(idx);
        let selected = StdSignalSet::from_bits_truncate(1 << idx);
        self.pending.remove(selected);
        Some((action, mask - StdSignalSet::NEVER_CAPTURE))
    }
    pub fn get_action(&mut self, sig: u32) -> SigAction {
        self.action[sig as usize]
    }
    pub fn replace_action(&mut self, sig: u32, mut new: SigAction) -> SigAction {
        debug_assert!(sig < 32);
        new.reset_never_capture(sig);
        core::mem::replace(&mut self.action[sig as usize], new)
    }
}

pub struct ThreadSignalManager {
    signal_pending: StdSignalSet,
    signal_mask: StdSignalSet,
}

impl ThreadSignalManager {
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            signal_pending: StdSignalSet::empty(),
            signal_mask: StdSignalSet::empty(),
        }
    }
    #[inline(always)]
    pub fn fork(&self) -> Self {
        Self {
            signal_pending: self.signal_pending,
            signal_mask: self.signal_mask,
        }
    }
    #[inline(always)]
    pub fn set_mask(&mut self, mut mask: StdSignalSet) {
        mask.remove_never_capture();
        self.signal_mask = mask;
    }
    #[inline(always)]
    pub fn mask(&self) -> StdSignalSet {
        self.signal_mask
    }
}
