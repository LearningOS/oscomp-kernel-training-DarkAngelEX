use core::ops::ControlFlow;

use alloc::vec::Vec;

use crate::{
    signal::{Action, SigAction, SignalSet, StdSignalSet, SIG_N, SIG_N_U32},
    sync::mutex::SpinNoIrqLock,
};

use super::{rtqueue::RTQueue, Sig};

const SEQ_MASK: usize = -2isize as usize;
/// 信箱序列号掩码, 只有最低位是0

const QUE_MASK: usize = 1;
/// 信号处理掩码, 只有最低位是1

pub struct ProcSignalManager {
    inner: SpinNoIrqLock<ProcSignalManagerInner>,
}

impl Default for ProcSignalManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcSignalManager {
    pub fn new() -> Self {
        Self {
            inner: SpinNoIrqLock::new(ProcSignalManagerInner::new()),
        }
    }
    /// 粗略判断是否存在信号
    #[inline(always)]
    pub fn have_signal(&self, recv_id: usize) -> bool {
        let inner = unsafe { self.inner.unsafe_get() };
        recv_id != inner.recv_id
    }
    /// 确定地判断是否存在信号
    pub fn have_signal_local(&self, mask: &SignalSet) -> bool {
        let inner = unsafe { self.inner.unsafe_get() };
        inner.can_take_std_signal(mask.std_signal()) || inner.can_take_rt_signal(mask)
    }
    pub fn recv_id(&self) -> usize {
        unsafe { self.inner.unsafe_get().recv_id }
    }
    /// 如果信号被接收了, 返回true
    pub fn receive(&self, sig: Sig) {
        self.inner.lock().receive(sig)
    }
    pub fn take_rt_signal(&self, mask: &SignalSet) -> ControlFlow<Sig> {
        stack_trace!();
        if unsafe { self.inner.unsafe_get().can_take_rt_signal(mask) } {
            self.inner.lock().take_rt_signal(mask)
        } else {
            ControlFlow::CONTINUE
        }
    }
    pub fn take_std_signal(&self, mask: StdSignalSet) -> ControlFlow<Sig> {
        stack_trace!();
        if unsafe { self.inner.unsafe_get().can_take_std_signal(mask) } {
            self.inner.lock().take_std_signal(mask)
        } else {
            ControlFlow::CONTINUE
        }
    }
    /// 返回sigaction
    pub fn get_sig_action(&self, sig: Sig) -> &SigAction {
        unsafe { self.inner.unsafe_get().get_sig_action(sig) }
    }
    /// 返回信号行为与阻塞信号集
    pub fn get_action(&self, sig: Sig) -> (Action, &SignalSet) {
        unsafe { self.inner.unsafe_get().get_action(sig) }
    }
    pub fn replace_action(&self, sig: Sig, new: &SigAction, old: &mut SigAction) {
        sig.check();
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
    ignore: SignalSet,
    recv_id: usize, // 每次增加2, 最低位一定是0
}

impl ProcSignalManagerInner {
    #[inline]
    pub fn new() -> Self {
        Self {
            pending: StdSignalSet::EMPTY,
            realtime: RTQueue::new(),
            action: SigAction::DEFAULT_SET,
            ignore: SigAction::DEFAULT_IGNORE,
            recv_id: 0,
        }
    }
    pub fn receive(&mut self, sig: Sig) {
        if self.ignore.get_bit(sig) {
            return;
        }
        match sig.0 {
            0..32 => self.pending.insert(StdSignalSet::from_sig(sig)),
            32..SIG_N_U32 => self.realtime.receive(sig),
            _ => (),
        }
        self.recv_id = self.recv_id.wrapping_add(2);
    }
    #[inline]
    pub fn can_take_std_signal(&self, mask: StdSignalSet) -> bool {
        !(self.pending & !mask & !self.ignore.std_signal()).is_empty()
    }
    pub fn take_std_signal(&mut self, mask: StdSignalSet) -> ControlFlow<Sig> {
        let can_fetch = self.pending & !mask & !self.ignore.std_signal();
        can_fetch.fetch().map_break(|a| {
            self.pending.clear_sig(a);
            a
        })
    }
    #[inline]
    pub fn can_take_rt_signal(&self, mask: &SignalSet) -> bool {
        self.realtime.can_fetch(mask)
    }
    pub fn take_rt_signal(&mut self, mask: &SignalSet) -> ControlFlow<Sig> {
        match self.realtime.fetch(mask) {
            Some(sig) => ControlFlow::Break(sig),
            None => ControlFlow::CONTINUE,
        }
    }
    pub fn get_sig_action(&self, sig: Sig) -> &SigAction {
        sig.check();
        &self.action[sig.0 as usize]
    }
    pub fn get_action(&self, sig: Sig) -> (Action, &SignalSet) {
        sig.check();
        let act = &self.action[sig.0 as usize];
        (act.get_action(sig), &act.mask)
    }
    pub fn replace_action(&mut self, sig: Sig, new: &SigAction, old: &mut SigAction) {
        sig.check();
        let dst = &mut self.action[sig.0 as usize];
        *old = *dst;
        *dst = *new;
        dst.reset_never_capture(sig);
        self.ignore = SignalSet::EMPTY;
        for i in 0..SIG_N_U32 {
            if self.action[i as usize].get_action(Sig(i)).ignore() {
                self.ignore.insert_bit(Sig(i));
            }
        }
    }
    pub fn fork(&self) -> Self {
        Self {
            pending: self.pending,
            realtime: self.realtime.fork(),
            action: self.action,
            ignore: self.ignore,
            recv_id: self.recv_id,
        }
    }
}

pub struct ThreadSignalManager {
    mailbox: SpinNoIrqLock<ThreadSignalMailbox>,
    /// 高63位 == send_id 时没有收到任何新信号，不需要锁。
    ///
    /// 最低位为1时
    recv_id: usize,
    std_pending: StdSignalSet,
    real_pending: RTQueue,
    signal_mask: SignalSet,
    /// 和进程控制块上的值完全相同时说明进程没收到新信号
    pub proc_recv_id: usize,
}

struct ThreadSignalMailbox {
    std: StdSignalSet,
    send_id: usize,
    realtime: Vec<Sig>,
}

impl Default for ThreadSignalManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ThreadSignalMailbox {
    pub const fn new() -> Self {
        Self {
            std: StdSignalSet::empty(),
            send_id: 0,
            realtime: Vec::new(),
        }
    }
    pub fn fork(&self) -> Self {
        Self {
            std: self.std,
            send_id: self.send_id,
            realtime: self.realtime.clone(),
        }
    }
    pub fn receive(&mut self, sig: Sig) {
        sig.check();
        debug_assert!(self.send_id & 1 == 0);
        match sig.0 {
            0..32 => {
                let mask = StdSignalSet::from_sig(sig);
                if self.std.contains(mask) {
                    return;
                }
                self.std.insert(mask);
                self.send_id = self.send_id.wrapping_add(2);
            }
            32..SIG_N_U32 => {
                self.realtime.push(sig);
                self.send_id = self.send_id.wrapping_add(2);
            }
            _ => (),
        }
    }
}

impl ThreadSignalManager {
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            mailbox: SpinNoIrqLock::new(ThreadSignalMailbox::new()),
            recv_id: 0,
            std_pending: StdSignalSet::empty(),
            real_pending: RTQueue::new(),
            signal_mask: SignalSet::EMPTY,
            proc_recv_id: 0,
        }
    }
    pub fn mask_changed(&mut self) {
        self.recv_id |= QUE_MASK;
    }
    #[inline(always)]
    pub fn fork(&self) -> Self {
        Self {
            mailbox: SpinNoIrqLock::new(self.mailbox.lock().fork()),
            recv_id: self.recv_id,
            std_pending: self.std_pending,
            real_pending: self.real_pending.fork(),
            signal_mask: self.signal_mask,
            proc_recv_id: self.proc_recv_id,
        }
    }
    #[inline]
    pub fn receive(&self, sig: Sig) {
        sig.check();
        self.mailbox.lock().receive(sig)
    }
    /// 从mailbox取出信号转移到排他内存
    ///
    /// mut是假的! mailbox是多线程变量
    pub fn fetch_mailbox(&mut self) {
        stack_trace!();
        // 无锁判断
        let send_id = unsafe { self.mailbox.unsafe_get().send_id };
        debug_assert!(send_id & 1 == 0);
        if self.recv_id & SEQ_MASK == send_id {
            return;
        }
        self.recv_id = send_id | (self.recv_id & QUE_MASK);

        let add = {
            let mut mailbox = self.mailbox.lock();
            self.std_pending |= mailbox.std;
            mailbox.std = StdSignalSet::empty();
            core::mem::take(&mut mailbox.realtime)
        }; // release lock here
        for sig in add {
            match sig.0 {
                0..32 => self.std_pending.insert(StdSignalSet::from_sig(sig)),
                32..SIG_N_U32 => self.real_pending.receive(sig),
                _ => (),
            }
        }
    }
    pub fn insert_local_flag(&mut self) {
        self.recv_id |= QUE_MASK
    }
    pub fn clear_local_flag(&mut self) {
        self.recv_id &= SEQ_MASK
    }
    pub fn update_proc_recv_id(&mut self, id: usize) {
        self.proc_recv_id = id;
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
    /// 是否应该处理信号
    #[inline(always)]
    pub fn have_signal(&self) -> bool {
        self.recv_id != unsafe { self.mailbox.unsafe_get().send_id }
    }
    /// 判断是否有未处理的本地信号
    pub fn have_signal_local(&self) -> bool {
        if !(self.std_pending & !self.signal_mask.std_signal()).is_empty() {
            return true;
        }
        self.real_pending.can_fetch(&self.signal_mask)
    }
    pub fn take_std_signal(&mut self) -> ControlFlow<Sig> {
        (self.std_pending & !self.signal_mask.std_signal())
            .fetch()
            .map_break(|a| {
                self.std_pending.clear_sig(a);
                a
            })
    }
    pub fn take_rt_signal(&mut self) -> ControlFlow<Sig> {
        match self.real_pending.fetch(&self.signal_mask) {
            Some(sig) => ControlFlow::Break(sig),
            None => ControlFlow::CONTINUE,
        }
    }
}
