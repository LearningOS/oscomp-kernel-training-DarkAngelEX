use crate::{
    signal::{Action, SigAction, SignalSet, StdSignalSet, SIG_N},
    sync::mutex::SpinNoIrqLock,
};

pub struct ProcSignalManager {
    inner: SpinNoIrqLock<ProcSignalManagerInner>,
}

impl ProcSignalManager {
    pub fn new() -> Self {
        Self {
            inner: SpinNoIrqLock::new(ProcSignalManagerInner::new()),
        }
    }
    pub fn receive(&self, sig: u32) {
        self.inner.lock().receive(sig)
    }
    /// 找到数字最小的信号并处理，返回处理方式和旧的阻塞集
    pub fn take_signal(&self, mask: SignalSet) -> Option<u32> {
        stack_trace!();
        self.inner.lock().take_signal(mask)
    }
    /// 找到数字最小的信号并处理，返回处理方式和旧的阻塞集
    pub fn take_std_signal(&self, mask: SignalSet) -> Option<u32> {
        stack_trace!();
        self.inner.lock().take_std_signal(mask)
    }
    /// 返回sigaction
    pub fn get_sig_action(&self, sig: u32) -> SigAction {
        unsafe { self.inner.unsafe_get().get_sig_action(sig) }
    }
    /// 返回信号行为与阻塞信号集
    pub fn get_action(&self, sig: u32) -> (Action, SignalSet) {
        unsafe { self.inner.unsafe_get().get_action(sig) }
    }
    pub fn replace_action(&self, sig: u32, new: SigAction) -> SigAction {
        debug_assert!(sig < 32);
        self.inner.lock().replace_action(sig, new)
    }
}

struct ProcSignalManagerInner {
    pending: SignalSet, // 等待处理的信号
    action: [SigAction; SIG_N],
}

impl ProcSignalManagerInner {
    pub fn new() -> Self {
        Self {
            pending: SignalSet::EMPTY,
            action: SigAction::DEFAULT_SET,
        }
    }
    pub fn receive(&mut self, sig: u32) {
        debug_assert!(sig < SIG_N as u32); // must check in syscall
        todo!()
    }
    // 找到数字最小的信号并处理，返回处理方式和旧的阻塞集
    pub fn take_signal(&mut self, _mask: SignalSet) -> Option<u32> {
        stack_trace!();
        todo!()
    }
    pub fn take_std_signal(&mut self, _mask: SignalSet) -> Option<u32> {
        todo!()
    }
    pub fn get_sig_action(&self, sig: u32) -> SigAction {
        self.action[sig as usize]
    }
    pub fn get_action(&self, sig: u32) -> (Action, SignalSet) {
        debug_assert!(sig < SIG_N as u32);
        let act = &self.action[sig as usize];
        (act.get_action(sig), act.mask)
    }
    pub fn replace_action(&mut self, sig: u32, mut new: SigAction) -> SigAction {
        debug_assert!(sig < 32);
        new.reset_never_capture(sig);
        core::mem::replace(&mut self.action[sig as usize], new)
    }
}

pub struct ThreadSignalManager {
    signal_pending: StdSignalSet,
    signal_mask: SignalSet,
}

impl ThreadSignalManager {
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            signal_pending: StdSignalSet::empty(),
            signal_mask: SignalSet::EMPTY,
        }
    }
    #[inline(always)]
    pub fn fork(&self) -> Self {
        Self {
            signal_pending: self.signal_pending,
            signal_mask: self.signal_mask,
        }
    }
    pub fn receive(&mut self, sig: u32) {
        debug_assert!(sig < SIG_N as u32); // must check in syscall
        todo!()
    }
    #[inline(always)]
    pub fn set_mask(&mut self, mut mask: SignalSet) {
        mask.remove_never_capture();
        self.signal_mask = mask;
    }
    #[inline(always)]
    pub fn mask(&self) -> SignalSet {
        self.signal_mask
    }
    pub fn take_std_signal(&mut self) -> Option<u32> {
        todo!()
    }
    pub fn take_signal(&mut self) -> Option<u32> {
        todo!()
    }
}
