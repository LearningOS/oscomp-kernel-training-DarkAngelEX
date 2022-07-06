pub mod context;
pub mod manager;
mod rtqueue;

use core::ops::ControlFlow;

use alloc::sync::Arc;

use crate::{
    config::USER_KRX_BEGIN,
    memory::user_ptr::UserInOutPtr,
    process::{thread::ThreadInner, Dead, Process},
    signal::context::SignalContext,
    sync::even_bus::Event,
    syscall::SysResult,
    user::check::UserCheck,
};

pub const SIG_N: usize = 64;
pub const SIG_N_U32: u32 = 64;
pub const SIG_N_BYTES: usize = SIG_N / 8;

pub const SIGHUP: usize = 1;
pub const SIGINT: usize = 2;
pub const SIGQUIT: usize = 3;
pub const SIGILL: usize = 4;
pub const SIGTRAP: usize = 5;
pub const SIGABRT: usize = 6;
pub const SIGBUS: usize = 7;
pub const SIGFPE: usize = 8;
pub const SIGKILL: usize = 9;
pub const SIGUSR1: usize = 10;
pub const SIGSEGV: usize = 11;
pub const SIGUSR2: usize = 12;
pub const SIGPIPE: usize = 13;
pub const SIGALRM: usize = 14;
pub const SIGTERM: usize = 15;
pub const SIGCHLD: usize = 17;
pub const SIGCONT: usize = 18;
pub const SIGSTOP: usize = 19;
pub const SIGTSTP: usize = 20;
pub const SIGTTIN: usize = 21;
pub const SIGTTOU: usize = 22;
pub const SIGURG: usize = 23;
pub const SIGXCPU: usize = 24;
pub const SIGXFSZ: usize = 25;
pub const SIGVTALRM: usize = 26;
pub const SIGPROF: usize = 27;
pub const SIGWINCH: usize = 28;
pub const SIGIO: usize = 29;
pub const SIGPWR: usize = 30;
pub const SIGSYS: usize = 31;

bitflags! {
    pub struct SA: usize {
        const RESTORER = 0x04000000;
    }
}

bitflags! {
    pub struct StdSignalSet: u32 {
        const SIGHUP    = 1 <<  1;   // 用户终端连接结束
        const SIGINT    = 1 <<  2;   // 程序终止 可能是Ctrl+C
        const SIGQUIT   = 1 <<  3;   // 类似SIGINT Ctrl+\
        const SIGILL    = 1 <<  4;   // 执行了非法指令 页错误 栈溢出
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
    pub const EMPTY: Self = Self::empty();
    pub const NEVER_CAPTURE: Self = Self::SIGKILL.union(Self::SIGSTOP);
    #[inline(always)]
    pub fn fetch_never_capture(&self) -> ControlFlow<u32> {
        if core::intrinsics::unlikely(self.contains(StdSignalSet::NEVER_CAPTURE)) {
            if self.contains(StdSignalSet::SIGKILL) {
                return ControlFlow::Break(SIGKILL as u32);
            }
            if self.contains(StdSignalSet::SIGSTOP) {
                return ControlFlow::Break(SIGSTOP as u32);
            }
        }
        ControlFlow::CONTINUE
    }
    pub fn fetch_segv(&self) -> ControlFlow<u32> {
        if self.contains(StdSignalSet::SIGSEGV) {
            ControlFlow::Break(SIGSEGV as u32)
        } else {
            ControlFlow::CONTINUE
        }
    }
    pub fn fetch(&self) -> ControlFlow<u32> {
        if self.is_empty() {
            return ControlFlow::CONTINUE;
        }
        self.fetch_never_capture()?;
        self.fetch_segv()?;
        let sig = self.bits.trailing_zeros();
        debug_assert!(sig < 32);
        ControlFlow::Break(sig)
    }
    pub fn clear_sig(&mut self, sig: u32) {
        if !SignalSet::is_never_capture_sig(sig) {
            self.remove(unsafe { StdSignalSet::from_bits_unchecked(1 << sig) })
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SignalSet(pub [usize; SIG_N / usize::BITS as usize]);

impl SignalSet {
    pub const EMPTY: Self = Self([0; _]);
    pub const NEVER_CAPTURE: Self = Self::never_capture();
    pub const fn never_capture() -> Self {
        let mut set = Self::EMPTY;
        set.0[0] = SIGKILL | SIGSTOP;
        set
    }
    #[inline(always)]
    pub const fn is_never_capture_sig(sig: u32) -> bool {
        debug_assert!(sig < SIG_N as u32);
        match sig as usize {
            SIGKILL | SIGSTOP => true,
            _ => false,
        }
    }
    pub fn std_signal(&self) -> StdSignalSet {
        unsafe { StdSignalSet::from_bits_unchecked(self.0[0] as u32) }
    }
    pub fn bytes(&self) -> impl Iterator<Item = u8> + '_ {
        self.0.iter().flat_map(|&a| usize::to_ne_bytes(a))
    }
    pub fn from_bytes(src: &[u8]) -> Self {
        let mut set = Self::EMPTY;
        set.read_from(src);
        set
    }
    pub fn read_from(&mut self, src: &[u8]) {
        const ULEN: usize = core::mem::size_of::<usize>();
        for (dst, src) in self.0.iter_mut().zip(src.chunks(ULEN)) {
            let mut buf = [0; ULEN];
            buf.copy_from_slice(src);
            *dst = usize::from_ne_bytes(buf);
        }
    }
    pub fn write_to(&self, dst: &mut [u8]) {
        for (dst, src) in dst.iter_mut().zip(self.bytes()) {
            *dst = src;
        }
    }
    /// cur = f(cur, sig)
    fn apply_all(&mut self, sig: &Self, mut f: impl FnMut(usize, usize) -> usize) {
        for (dst, src) in self.0.iter_mut().zip(sig.0.iter().copied()) {
            *dst = f(*dst, src)
        }
    }
    fn check_any(&self, sig: &Self, mut f: impl FnMut(usize, usize) -> bool) -> bool {
        self.0
            .iter()
            .copied()
            .zip(sig.0.iter().copied())
            .any(|(a, b)| f(a, b))
    }
    pub fn coniatn(&self, sig: &Self) -> bool {
        self.check_any(sig, |a, b| a | b != 0)
    }
    /// A & !B != 0
    pub fn can_fetch(&self, mask: &Self) -> bool {
        self.check_any(mask, |a, b| a & !b != 0)
    }
    /// A |= B
    pub fn insert(&mut self, sigs: &Self) {
        self.apply_all(sigs, |a, b| a | b);
    }
    /// 将第place个bit置为1
    pub fn insert_bit(&mut self, place: usize) {
        self.0[place / usize::BITS as usize] |= 1 << (place % usize::BITS as usize);
    }
    pub fn remove_bit(&mut self, place: usize) {
        self.0[place / usize::BITS as usize] &= !(1 << (place % usize::BITS as usize));
    }
    pub fn get_bit(&self, place: usize) -> bool {
        (self.0[place / usize::BITS as usize] & (1 << (place % usize::BITS as usize))) != 0
    }
    /// A &= !B
    pub fn remove(&mut self, sigs: &Self) {
        self.apply_all(sigs, |a, b| a & !b);
    }
    #[inline(always)]
    pub fn remove_never_capture(&mut self) {
        self.remove(&Self::NEVER_CAPTURE)
    }
    #[inline(always)]
    pub fn contain_never_capture(&self) -> bool {
        self.coniatn(&Self::NEVER_CAPTURE)
    }
    pub fn bit_fold<A>(&self, mut acc: A, mut f: impl FnMut(u32, A) -> A) -> A {
        for (i, mut src) in self.0.iter().copied().enumerate() {
            if src == 0 {
                continue;
            }
            let mut p = i as u32 * usize::BITS;
            loop {
                let t = src.trailing_zeros();
                p += t;
                src >>= t;
                if src == 0 {
                    break;
                }
                acc = f(p, acc);
            }
        }
        return acc;
    }
}

const SIG_DFL: usize = 0; // 默认信号
const SIG_IGN: usize = 1; // 忽略信号
const SIG_ERR: usize = usize::MAX; // 错误值

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SigAction {
    pub handler: usize,
    pub flags: SA,
    pub restorer: usize,
    pub mask: SignalSet,
}

impl SigAction {
    pub fn zeroed() -> Self {
        unsafe { core::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SignalStack {
    ss_sp: UserInOutPtr<u8>,
    ss_flags: u32,
    ss_size: usize,
}

pub enum Action {
    Abort,
    Ignore,
    Handler(usize, usize),
}

impl SigAction {
    pub const DEFAULT: Self = Self {
        handler: SIG_DFL,
        flags: SA::empty(),
        restorer: 0,
        mask: SignalSet::EMPTY,
    };
    pub const DEFAULT_SET: [Self; 64] = [Self::DEFAULT; 64];
    pub fn default_restorer() -> usize {
        extern "C" {
            fn __kload_begin();
            fn __user_signal_entry_begin();
        }
        let offset = __user_signal_entry_begin as usize - __kload_begin as usize;
        USER_KRX_BEGIN + offset
    }
    #[inline(always)]
    pub fn defalut_action(sig: u32) -> Action {
        match sig as usize {
            SIGCHLD | SIGCONT | SIGURG => Action::Ignore,
            0..32 => Action::Abort,
            32..SIG_N => Action::Ignore,
            e => panic!("error sig: {}", e),
        }
    }
    pub fn get_action(&self, sig: u32) -> Action {
        match self.handler {
            SIG_DFL => Self::defalut_action(sig),
            SIG_IGN => Action::Ignore,
            SIG_ERR => Action::Abort,
            handler => {
                let restorer = if self.flags.contains(SA::RESTORER) {
                    self.restorer
                } else {
                    SigAction::default_restorer()
                };
                Action::Handler(handler, restorer)
            }
        }
    }
    #[inline(always)]
    pub fn reset_never_capture(&mut self, sig: u32) {
        if SignalSet::is_never_capture_sig(sig) {
            self.handler = SIG_DFL;
        }
        self.mask.remove_never_capture();
    }
    pub fn show(&self) {
        println!("handler:  {:#x}", self.handler);
        println!("flags:    {:#x}", self.flags);
        println!("restorer: {:#x}", self.restorer);
        println!("mask:     {:#x?}", self.mask.0);
    }
}

pub fn send_signal(process: Arc<Process>, sig: u32) -> Result<(), Dead> {
    process.signal_manager.receive(sig);
    process.event_bus.set(Event::RECEIVE_SIGNAL)?;
    Ok(())
}

pub async fn handle_signal(thread: &mut ThreadInner, process: &Process) -> Result<(), Dead> {
    stack_trace!();
    let tsm = &mut thread.signal_manager;
    let psm = &process.signal_manager;
    tsm.fetch_mailbox();
    let mask = *tsm.mask();
    let mut take_sig_fn = || {
        tsm.take_std_signal()?;
        psm.take_std_signal(mask.std_signal())?;
        tsm.take_rt_signal()?;
        psm.take_rt_signal(&mask)?;
        ControlFlow::CONTINUE
    };
    let signal = match take_sig_fn().break_value() {
        Some(s) => s,
        None => return Ok(()),
    };
    // 找到了一个待处理信号
    assert!(signal < SIG_N as u32);
    let (act, sig_mask) = psm.get_action(signal);
    let (handler, ra) = match act {
        Action::Abort => return Err(Dead),
        Action::Ignore => return Ok(()),
        Action::Handler(h, ra) => (h, ra),
    };
    // 使用handler的信号处理
    debug_assert!(![0, 1, usize::MAX].contains(&handler));
    let old_mask = mask;
    let mut new_mask = mask;
    new_mask.insert(sig_mask);
    new_mask.insert_bit(signal as usize);
    tsm.set_mask(&new_mask);
    let old_scxptr = thread.scx_ptr;
    let uk_cx = &mut thread.uk_context;
    let mut sp = uk_cx.sp();
    sp -= 128; // red zone
    sp -= core::mem::size_of::<SignalContext>();
    sp -= sp & 15; // align 16 bytes
    let scx_ptr: UserInOutPtr<SignalContext> = UserInOutPtr::from_usize(sp);
    sp -= 16;
    let scx = UserCheck::new(process)
        .writable_value(scx_ptr)
        .await
        .map_err(|_e| Dead)?;
    scx.access_mut()[0].load(uk_cx, old_scxptr, old_mask);
    uk_cx.set_user_a0(signal as usize);
    uk_cx.set_user_sp(sp);
    uk_cx.set_user_ra(ra);
    uk_cx.set_user_sepc(handler);
    thread.scx_ptr = scx_ptr;
    Ok(())
}

pub async fn sigreturn(thread: &mut ThreadInner, process: &Process) -> SysResult {
    stack_trace!();
    debug_assert!(!thread.scx_ptr.is_null());
    let scx = UserCheck::new(process)
        .readonly_value(thread.scx_ptr)
        .await?;
    match scx.access()[0].store(&mut thread.uk_context) {
        (scx_ptr, mask) => {
            thread.scx_ptr = scx_ptr;
            thread.signal_manager.set_mask(mask);
        }
    }
    Ok(thread.uk_context.a0())
}
