use alloc::{collections::LinkedList, sync::Arc};

use crate::{
    memory::address::UserAddr,
    process::{Dead, Process},
    sync::even_bus::Event,
};

#[allow(dead_code, clippy::upper_case_acronyms)]
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

#[repr(C)]
#[derive(Clone, Copy)]
pub union SigActionUnion {
    handler: UserAddr,   // (u32) -> ()
    sigaction: UserAddr, // (u32, *siginfo_t, *()) -> ()
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SigAction {
    union: SigActionUnion,
    mask: StdSignalSet, //
    flags: u32,         //
    restorer: UserAddr, // () -> ()
}

pub struct SignalPack {
    signal: StardardSignal,
}

pub fn send_signal(process: Arc<Process>, signal_set: StdSignalSet) -> Result<(), Dead> {
    let mut signal_queue = LinkedList::new();
    for i in 1..31u8 {
        if signal_set.bits() & (1 << i) != 0 {
            signal_queue.push_back(unsafe { core::mem::transmute(i) })
        }
    }
    process.alive_then(|a| a.signal_queue.append(&mut signal_queue))?;
    if !signal_set.is_empty() {
        process
            .event_bus
            .lock(place!())
            .set(Event::RECEIVE_SIGNAL)?;
    }
    Ok(())
}

pub fn handle_signal() {
    todo!()
}
