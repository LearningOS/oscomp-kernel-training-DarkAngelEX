use alloc::sync::Arc;

use crate::{
    process::{Dead, Process},
    sync::even_bus::Event,
};

#[allow(dead_code, clippy::upper_case_acronyms)]
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StardardSignal {
    SIGHUP = 1,
    SIGINT = 2,
    SIGQUIT = 3,
    SIGILL = 4,
    SIGTRAP = 5,
    SIGABRT = 6,
    SIGBUS = 7,
    SIGFPE = 8,
    SIGKILL = 9,
    SIGUSR1 = 10,
    SIGSEGV = 11,
    SIGUSR2 = 12,
    SIGPIPE = 13,
    SIGALRM = 14,
    SIGTERM = 15,

    SIGCHLD = 17,
    SIGCONT = 18,
    SIGSTOP = 19,
    SIGTSTP = 20,
    SIGTTIN = 21,
    SIGTTOU = 22,
    SIGURG = 23,
    SIGXCPU = 24,
    SIGXFSZ = 25,
    SIGVTALRM = 26,
    SIGPROF = 27,
    SIGWINCH = 28,
    SIGIO = 29,
    SIGPWR = 30,
    SIGSYS = 31,
}

bitflags! {
    pub struct StdSignalSet: u32 {
        const SIGHUP    = 1 <<  1;   // 用户终端连接结束
        const SIGINT    = 1 <<  2;   // 程序终止 可能是Ctrl+C
        const SIGQUIT   = 1 <<  3;   // 类似SIGINT Ctrl+\
        const SIGILL    = 1 <<  4;   // 执行了非法指令
        const SIGTRAP   = 1 <<  5;   // 断点指令产生 debugger使用
        const SIGABRT   = 1 <<  6;   // abort函数产生
        const SIGBUS    = 1 <<  7;   // 非法地址或地址未对齐
        const SIGFPE    = 1 <<  8;   // 致命算数运算错误，浮点或溢出或除以0
        const SIGKILL   = 1 <<  9;   // 强制立刻结束程序执行
        const SIGUSR1   = 1 << 10;   // 用户保留1
        const SIGSEGV   = 1 << 11;   // 试图读写未分配或无权限的地址
        const SIGUSR2   = 1 << 12;   // 用户保留2
        const SIGPIPE   = 1 << 13;   // 管道破裂，没有读管道
        const SIGALRM   = 1 << 14;   // 时钟定时信号
        const SIGTERM   = 1 << 15;   // 程序结束信号，用来要求程序自己正常退出
        const SIGSTKFLT = 1 << 16;   //
        const SIGCHLD   = 1 << 17;   // 子进程结束时父进程收到这个信号
        const SIGCONT   = 1 << 18;   // 让停止的进程继续执行，不能阻塞 例如重新显示提示符
        const SIGSTOP   = 1 << 19;   // 暂停进程 不能阻塞或忽略
        const SIGTSTP   = 1 << 20;   // 暂停进程 可处理或忽略 Ctrl+Z
        const SIGTTIN   = 1 << 21;   // 当后台作业要从用户终端读数据时, 该作业中的所有进程会收到SIGTTIN信号. 缺省时这些进程会停止执行
        const SIGTTOU   = 1 << 22;   // 类似于SIGTTIN, 但在写终端(或修改终端模式)时收到.
        const SIGURG    = 1 << 23;   // 有"紧急"数据或out-of-band数据到达socket时产生.
        const SIGXCPU   = 1 << 24;   // 超过CPU时间资源限制 可以由getrlimit/setrlimit来读取/改变。
        const SIGXFSZ   = 1 << 25;   // 进程企图扩大文件以至于超过文件大小资源限制
        const SIGVTALRM = 1 << 26;   // 虚拟时钟信号, 类似于SIGALRM, 但是计算的是该进程占用的CPU时间.
        const SIGPROF   = 1 << 27;   // 类似于SIGALRM/SIGVTALRM, 但包括该进程用的CPU时间以及系统调用的时间
        const SIGWINCH  = 1 << 28;   // 窗口大小改变时发出
        const SIGIO     = 1 << 29;   // 文件描述符准备就绪, 可以开始进行输入/输出操作.
        const SIGPWR    = 1 << 30;   // Power failure
        const SIGSYS    = 1 << 31;   // 非法的系统调用
    }
}

impl StdSignalSet {
    pub fn from_bytes(a: &[u8]) -> Self {
        let mut r = StdSignalSet::empty();
        for (i, &v) in a.iter().take(4).enumerate() {
            r.bits |= (v as u32) << (i * 8);
        }
        r
    }
    pub fn as_bytes(&self) -> [u8; 4] {
        self.bits.to_le_bytes()
    }
    pub fn write_to(&self, dst: &mut [u8]) {
        dst.copy_from_slice(&self.as_bytes())
    }
    #[inline(always)]
    pub const fn is_never_capture_sig(sig: u32) -> bool {
        debug_assert!(sig < 32);
        match sig {
            9 | 19 => true,
            _ => false,
        }
    }
    /// 禁止捕获或忽略的信号
    pub const NEVER_CAPTURE: Self = Self::SIGKILL.union(Self::SIGSTOP);
    #[inline(always)]
    pub fn remove_never_capture(&mut self) {
        self.remove(Self::NEVER_CAPTURE)
    }
    pub fn contain_never_capture(&self) -> bool {
        self.contains(Self::NEVER_CAPTURE)
    }
}

#[derive(Clone, Copy)]
pub struct SignalSet {
    set: StdSignalSet,
}
impl SignalSet {
    pub const fn empty() -> Self {
        Self {
            set: StdSignalSet::empty(),
        }
    }
    pub fn as_bytes(&self) -> [u8; 4] {
        let mut s = [0; 4];
        for (i, b) in s.iter_mut().enumerate() {
            *b = (self.set.bits() >> i * 8) as u8;
        }
        s
    }
    pub fn write_to(&self, dst: &mut [u8]) {
        for (dst, src) in dst.iter_mut().zip(self.as_bytes()) {
            *dst = src;
        }
    }
    fn set_sigs(&mut self, sigs: StdSignalSet) {
        self.set |= sigs;
    }
    fn clear_sigs(&mut self, sigs: StdSignalSet) {
        self.set &= !sigs;
    }
    fn clear_ignore(&mut self) {
        self.clear_sigs(StdSignalSet::SIGKILL | StdSignalSet::SIGSTOP);
    }
    pub fn set_bit(&mut self, src: &[u8]) {
        self.set_sigs(StdSignalSet::from_bytes(src));
        self.clear_ignore();
    }
    pub fn clear_bit(&mut self, src: &[u8]) {
        self.clear_sigs(StdSignalSet::from_bytes(src));
        self.clear_ignore();
    }
    pub fn set(&mut self, src: &[u8]) {
        self.set = StdSignalSet::from_bytes(src);
        self.clear_ignore();
    }
}

const SIG_DFL: usize = 0; // 默认信号
const SIG_IGN: usize = 1; // 忽略信号
const SIG_ERR: usize = usize::MAX; // 错误值

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SigAction {
    pub handler: usize,
    pub mask: StdSignalSet,
    pub restorer: usize,
    pub flags: usize,
}

pub enum Action {
    Abort,
    Ignore,
    Handler(usize),
}

impl SigAction {
    pub const DEFAULT: Self = Self {
        handler: SIG_DFL,
        mask: StdSignalSet::empty(),
        restorer: 0,
        flags: 0,
    };
    pub const DEFAULT_SET: [Self; 32] = [Self::DEFAULT; 32];
    #[inline(always)]
    pub fn defalut_action(sig: u32) -> Action {
        use StardardSignal::*;
        assert!(sig < 32);
        if [SIGCHLD, SIGCONT, SIGURG]
            .iter()
            .copied()
            .map(|a| a as u32)
            .any(|a| a == sig)
        {
            Action::Ignore
        } else {
            Action::Abort
        }
    }
    pub fn get_action(&self, sig: u32) -> Action {
        match self.handler {
            SIG_DFL => Self::defalut_action(sig),
            SIG_IGN => Action::Ignore,
            SIG_ERR => Action::Abort,
            handler => Action::Handler(handler),
        }
    }
    #[inline(always)]
    pub fn reset_never_capture(&mut self, sig: u32) {
        if StdSignalSet::is_never_capture_sig(sig) {
            self.handler = SIG_DFL;
        }
        self.mask.remove_never_capture();
    }
    pub fn show(&self) {
        println!("handler:  {:#x}", self.handler);
        println!("mask:     {:#x}", self.mask);
        println!("restorer: {:#x}", self.restorer);
        println!("flags:    {:#x}", self.flags);
    }
}

pub struct SignalPack {
    signal: StardardSignal,
}

pub fn send_signal(process: Arc<Process>, signal_set: StdSignalSet) -> Result<(), Dead> {
    process.alive_then(move |a| a.signal_manager.send(signal_set))?;
    if !signal_set.is_empty() {
        process.event_bus.set(Event::RECEIVE_SIGNAL)?;
    }
    Ok(())
}

pub fn handle_signal() {
    todo!()
}
