use core::ops::ControlFlow;

use alloc::collections::VecDeque;

use crate::{
    signal::{Action, SigAction, SignalSet, StdSignalSet, SIG_N, SIG_N_U32},
    sync::mutex::SpinNoIrqLock,
};

use super::rtqueue::RTQueue;

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
    pub fn take_rt_signal(&self, mask: &SignalSet) -> ControlFlow<u32> {
        stack_trace!();
        if unsafe { self.inner.unsafe_get().can_take_rt_signal(mask) } {
            self.inner.lock().take_rt_signal(mask)
        } else {
            ControlFlow::CONTINUE
        }
    }
    pub fn take_std_signal(&self, mask: StdSignalSet) -> ControlFlow<u32> {
        stack_trace!();
        if unsafe { self.inner.unsafe_get().can_take_std_signal(mask) } {
            self.inner.lock().take_std_signal(mask)
        } else {
            ControlFlow::CONTINUE
        }
    }
    /// 返回sigaction
    pub fn get_sig_action(&self, sig: u32) -> SigAction {
        unsafe { self.inner.unsafe_get().get_sig_action(sig) }
    }
    /// 返回信号行为与阻塞信号集
    pub fn get_action(&self, sig: u32) -> (Action, &SignalSet) {
        unsafe { self.inner.unsafe_get().get_action(sig) }
    }
    pub fn replace_action(&self, sig: u32, new: &SigAction, old: &mut SigAction) {
        debug_assert!(sig < SIG_N_U32);
        self.inner.lock().replace_action(sig, new, old)
    }
    pub fn reset(&self) {
        *self.inner.lock() = ProcSignalManagerInner::new();
    }
    pub fn fork(&self) -> Self {
        Self {
            inner: SpinNoIrqLock::new(self.inner.lock().fork()),
        }
    }
}

struct ProcSignalManagerInner {
    pending: StdSignalSet, // 等待处理的信号
    realtime: RTQueue,
    action: [SigAction; SIG_N],
}

impl ProcSignalManagerInner {
    #[inline]
    pub fn new() -> Self {
        Self {
            pending: StdSignalSet::EMPTY,
            realtime: RTQueue::new(),
            action: SigAction::DEFAULT_SET,
        }
    }
    pub fn receive(&mut self, sig: u32) {
        debug_assert!(sig < SIG_N as u32); // must check in syscall
        match sig {
            0..32 => self
                .pending
                .insert(StdSignalSet::from_bits_truncate(1 << sig)),
            32..SIG_N_U32 => self.realtime.receive(sig),
            _ => (),
        }
    }
    pub fn can_take_std_signal(&self, mask: StdSignalSet) -> bool {
        !(self.pending & !mask).is_empty()
    }
    pub fn take_std_signal(&mut self, mask: StdSignalSet) -> ControlFlow<u32> {
        (self.pending & !mask).fetch().map_break(|a| {
            self.pending.clear_sig(a);
            a
        })
    }
    pub fn can_take_rt_signal(&self, mask: &SignalSet) -> bool {
        self.realtime.can_fetch(mask)
    }
    pub fn take_rt_signal(&mut self, mask: &SignalSet) -> ControlFlow<u32> {
        match self.realtime.fetch(mask) {
            Some(sig) => ControlFlow::Break(sig),
            None => ControlFlow::CONTINUE,
        }
    }
    pub fn get_sig_action(&self, sig: u32) -> SigAction {
        self.action[sig as usize]
    }
    pub fn get_action(&self, sig: u32) -> (Action, &SignalSet) {
        debug_assert!(sig < SIG_N_U32);
        let act = &self.action[sig as usize];
        (act.get_action(sig), &act.mask)
    }
    pub fn replace_action(&mut self, sig: u32, new: &SigAction, old: &mut SigAction) {
        debug_assert!(sig < SIG_N_U32);
        let dst = &mut self.action[sig as usize];
        *old = *dst;
        *dst = *new;
        dst.reset_never_capture(sig);
    }
    pub fn fork(&self) -> Self {
        Self {
            pending: self.pending,
            realtime: self.realtime.fork(),
            action: self.action,
        }
    }
}

pub struct ThreadSignalManager {
    mailbox: SpinNoIrqLock<ThreadSignalMailbox>,
    recv_id: usize, // 当 recv_id == send_id 时没有收到任何新信号，不需要锁。
    std_pending: StdSignalSet,
    real_pending: RTQueue,
    signal_mask: SignalSet,
}

struct ThreadSignalMailbox {
    std: StdSignalSet,
    send_id: usize,
    realtime: VecDeque<u32>,
}
impl ThreadSignalMailbox {
    pub fn new() -> Self {
        Self {
            std: StdSignalSet::empty(),
            send_id: 0,
            realtime: VecDeque::new(),
        }
    }
    pub fn fork(&self) -> Self {
        Self {
            std: self.std,
            send_id: self.send_id,
            realtime: self.realtime.clone(),
        }
    }
    pub fn receive(&mut self, sig: u32) {
        match sig {
            1..32 => {
                let mask = StdSignalSet::from_bits_truncate(1u32 << sig);
                if self.std.contains(mask) {
                    return;
                }
                self.std.insert(mask);
                self.send_id += 1;
            }
            34..SIG_N_U32 => {
                self.realtime.push_back(sig);
                self.send_id += 1;
            }
            _ => (),
        }
    }
}

impl ThreadSignalManager {
    #[inline(always)]
    pub fn new() -> Self {
        Self {
            mailbox: SpinNoIrqLock::new(ThreadSignalMailbox::new()),
            recv_id: 0,
            std_pending: StdSignalSet::empty(),
            real_pending: RTQueue::new(),
            signal_mask: SignalSet::EMPTY,
        }
    }
    #[inline(always)]
    pub fn fork(&self) -> Self {
        Self {
            mailbox: SpinNoIrqLock::new(self.mailbox.lock().fork()),
            recv_id: self.recv_id,
            std_pending: self.std_pending,
            real_pending: self.real_pending.fork(),
            signal_mask: self.signal_mask,
        }
    }
    pub fn receive(&self, sig: u32) {
        debug_assert!(sig < SIG_N as u32); // must check in syscall
        self.mailbox.lock().receive(sig)
    }
    /// 从mailbox取出信号转移到排他内存
    ///
    /// mut是假的! mailbox是多线程变量
    pub fn fetch_mailbox(&mut self) {
        // 无锁判断
        let send_id = unsafe { self.mailbox.unsafe_get().send_id };
        if send_id == self.recv_id {
            return;
        }
        self.recv_id = send_id;

        let add = {
            let mut mailbox = self.mailbox.lock();
            self.std_pending |= mailbox.std;
            mailbox.std = StdSignalSet::empty();
            core::mem::take(&mut mailbox.realtime)
        }; // release lock here
        for sig in add {
            match sig {
                sig @ 1..32 => self
                    .std_pending
                    .insert(StdSignalSet::from_bits_truncate(sig)),
                34..SIG_N_U32 => self.real_pending.receive(sig),
                _ => (),
            }
        }
    }
    #[inline(always)]
    pub fn set_mask(&mut self, mask: &SignalSet) {
        self.signal_mask = *mask;
        self.signal_mask.remove_never_capture();
    }
    #[inline(always)]
    pub fn mask(&self) -> &SignalSet {
        &self.signal_mask
    }
    #[inline(always)]
    pub fn mask_mut(&mut self) -> &mut SignalSet {
        &mut self.signal_mask
    }
    pub fn take_std_signal(&mut self) -> ControlFlow<u32> {
        (self.std_pending & !self.signal_mask.std_signal())
            .fetch()
            .map_break(|a| {
                self.std_pending.clear_sig(a);
                a
            })
    }
    pub fn take_rt_signal(&mut self) -> ControlFlow<u32> {
        match self.real_pending.fetch(&self.signal_mask) {
            Some(sig) => ControlFlow::Break(sig),
            None => ControlFlow::CONTINUE,
        }
    }
}
