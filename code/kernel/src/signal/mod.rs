pub mod context;
pub mod manager;
mod rtqueue;

use core::{fmt::Debug, ops::ControlFlow};

use alloc::sync::Arc;
use ftl_util::error::SysError;

use crate::{
    config::USER_KRX_BEGIN,
    memory::user_ptr::UserInOutPtr,
    process::{thread::ThreadInner, Dead, Process},
    signal::context::SignalContext,
    sync::even_bus::Event,
    syscall::SysResult,
    user::check::UserCheck,
    xdebug::PRINT_SYSCALL_ALL,
};

/// 为了提高效率, Sig相比信号值都减去了1
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Sig(pub u32);
impl Sig {
    /// 减去 1 来提高 mask 的运算速度
    #[inline(always)]
    pub fn from_user(v: u32) -> Result<Self, SysError> {
        if v > 0 && v <= SIG_N_U32 {
            Ok(Self(v - 1))
        } else {
            Err(SysError::EINVAL)
        }
    }
    #[inline(always)]
    pub fn to_user(self) -> u32 {
        self.0 + 1
    }
    /// 使用 assume 在 release 模式下为编译器提供更强的优化能力
    #[inline(always)]
    pub fn check(self) {
        debug_assert!(self.0 < SIG_N_U32);
        unsafe { core::intrinsics::assume(self.0 < SIG_N_U32) }
    }
}

impl Debug for Sig {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("Sig").field(&(self.0 + 1)).finish()
    }
}

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
        const SIGHUP    = 1 << ( 1 - 1);   // 用户终端连接结束
        const SIGINT    = 1 << ( 2 - 1);   // 程序终止 可能是Ctrl+C
        const SIGQUIT   = 1 << ( 3 - 1);   // 类似SIGINT Ctrl+\
        const SIGILL    = 1 << ( 4 - 1);   // 执行了非法指令 页错误 栈溢出
        const SIGTRAP   = 1 << ( 5 - 1);   // 断点指令产生 debugger使用
        const SIGABRT   = 1 << ( 6 - 1);   // abort函数产生
        const SIGBUS    = 1 << ( 7 - 1);   // 非法地址或地址未对齐
        const SIGFPE    = 1 << ( 8 - 1);   // 致命算数运算错误，浮点或溢出或除以0
        const SIGKILL   = 1 << ( 9 - 1);   // 强制立刻结束程序执行
        const SIGUSR1   = 1 << (10 - 1);   // 用户保留1
        const SIGSEGV   = 1 << (11 - 1);   // 试图读写未分配或无权限的地址
        const SIGUSR2   = 1 << (12 - 1);   // 用户保留2
        const SIGPIPE   = 1 << (13 - 1);   // 管道破裂，没有读管道
        const SIGALRM   = 1 << (14 - 1);   // 时钟定时信号
        const SIGTERM   = 1 << (15 - 1);   // 程序结束信号，用来要求程序自己正常退出
        const SIGSTKFLT = 1 << (16 - 1);   //
        const SIGCHLD   = 1 << (17 - 1);   // 子进程结束时父进程收到这个信号
        const SIGCONT   = 1 << (18 - 1);   // 让停止的进程继续执行，不能阻塞 例如重新显示提示符
        const SIGSTOP   = 1 << (19 - 1);   // 暂停进程 不能阻塞或忽略
        const SIGTSTP   = 1 << (20 - 1);   // 暂停进程 可处理或忽略 Ctrl+Z
        const SIGTTIN   = 1 << (21 - 1);   // 当后台作业要从用户终端读数据时, 该作业中的所有进程会收到SIGTTIN信号. 缺省时这些进程会停止执行
        const SIGTTOU   = 1 << (22 - 1);   // 类似于SIGTTIN, 但在写终端(或修改终端模式)时收到.
        const SIGURG    = 1 << (23 - 1);   // 有"紧急"数据或out-of-band数据到达socket时产生.
        const SIGXCPU   = 1 << (24 - 1);   // 超过CPU时间资源限制 可以由getrlimit/setrlimit来读取/改变。
        const SIGXFSZ   = 1 << (25 - 1);   // 进程企图扩大文件以至于超过文件大小资源限制
        const SIGVTALRM = 1 << (26 - 1);   // 虚拟时钟信号, 类似于SIGALRM, 但是计算的是该进程占用的CPU时间.
        const SIGPROF   = 1 << (27 - 1);   // 类似于SIGALRM/SIGVTALRM, 但包括该进程用的CPU时间以及系统调用的时间
        const SIGWINCH  = 1 << (28 - 1);   // 窗口大小改变时发出
        const SIGIO     = 1 << (29 - 1);   // 文件描述符准备就绪, 可以开始进行输入/输出操作.
        const SIGPWR    = 1 << (30 - 1);   // Power failure
        const SIGSYS    = 1 << (31 - 1);   // 非法的系统调用
        const SIGTIMER  = 1 << (32 - 1);   // 非法的系统调用
    }
}

impl StdSignalSet {
    pub const EMPTY: Self = Self::empty();
    pub const NEVER_CAPTURE: Self = Self::SIGKILL.union(Self::SIGSTOP);
    pub fn from_sig(sig: Sig) -> Self {
        match sig.0 {
            0..32 => Self::from_bits_truncate(1 << sig.0),
            _ => panic!(),
        }
    }
    #[inline(always)]
    pub fn fetch_never_capture(&self) -> ControlFlow<Sig> {
        if core::intrinsics::unlikely(self.contains(StdSignalSet::NEVER_CAPTURE)) {
            if self.contains(StdSignalSet::SIGKILL) {
                return ControlFlow::Break(Sig::from_user(SIGKILL as u32).unwrap());
            }
            if self.contains(StdSignalSet::SIGSTOP) {
                return ControlFlow::Break(Sig::from_user(SIGSTOP as u32).unwrap());
            }
        }
        ControlFlow::CONTINUE
    }
    pub fn fetch_segv(&self) -> ControlFlow<Sig> {
        if self.contains(StdSignalSet::SIGSEGV) {
            ControlFlow::Break(Sig::from_user(SIGSEGV as u32).unwrap())
        } else {
            ControlFlow::CONTINUE
        }
    }
    pub fn fetch(&self) -> ControlFlow<Sig> {
        if self.is_empty() {
            return ControlFlow::CONTINUE;
        }
        self.fetch_never_capture()?;
        self.fetch_segv()?;
        let sig = Sig(self.bits.trailing_zeros());
        debug_assert!(sig.0 < 32);
        ControlFlow::Break(sig)
    }
    pub fn clear_sig(&mut self, sig: Sig) {
        if !SignalSet::is_never_capture_sig(sig) {
            self.remove(StdSignalSet::from_sig(sig))
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
        set.0[0] = 1 << SIGKILL - 1 | 1 << SIGSTOP - 1;
        set
    }
    #[inline(always)]
    pub const fn is_never_capture_sig(sig: Sig) -> bool {
        match (sig.0 + 1) as usize {
            SIGKILL | SIGSTOP => true,
            _ => false,
        }
    }
    pub fn std_signal(&self) -> StdSignalSet {
        StdSignalSet::from_bits_truncate(self.0[0] as u32)
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
    pub fn insert_bit(&mut self, sig: Sig) {
        sig.check();
        let sig = sig.0 as usize;
        self.0[sig / usize::BITS as usize] |= 1 << (sig % usize::BITS as usize);
    }
    pub fn remove_bit(&mut self, sig: Sig) {
        sig.check();
        let sig = sig.0 as usize;
        self.0[sig / usize::BITS as usize] &= !(1 << (sig % usize::BITS as usize));
    }
    pub fn get_bit(&self, sig: Sig) -> bool {
        sig.check();
        let sig = sig.0 as usize;
        (self.0[sig / usize::BITS as usize] & (1 << (sig % usize::BITS as usize))) != 0
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
    pub fn reset_never_capture(&mut self, sig: Sig) {
        if SignalSet::is_never_capture_sig(sig) {
            self.handler = SIG_DFL;
        }
        self.mask.remove_never_capture();
    }
    pub fn show(&self) {
        println!("handler:  {:#x}", self.handler);
        println!("flags:    {:#x}", self.flags);
        println!("restorer: {:#x}", self.restorer);
        println!("mask:     {:#x}", self.mask.0[0]);
    }
}

pub fn send_signal(process: Arc<Process>, sig: Sig) -> Result<(), Dead> {
    process.signal_manager.receive(sig);
    process.event_bus.set(Event::RECEIVE_SIGNAL)?;
    Ok(())
}

static mut HANDLE_CNT: usize = 0;

///
/// signal handler包含如下参数: (sig, si, ctx), 其中:
///
///     sig: 信号ID
///     si:  siginfo_t  cancel_handler用不上
///     ctx: ucontext_t 保存的原上下文
///
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
    if PRINT_SYSCALL_ALL {
        println!(
            "handle_signal - find signal: {:?} sepc: {:#x}",
            signal, thread.uk_context.user_sepc
        );
    }
    unsafe {
        HANDLE_CNT += 1;
        if HANDLE_CNT > 30 {
            panic!("handle too many signal!");
        }
    }
    let (act, sig_mask) = psm.get_action(signal);
    let (handler, ra) = match act {
        Action::Abort => return Err(Dead),
        Action::Ignore => return Ok(()),
        Action::Handler(h, ra) => (h, ra),
    };
    if PRINT_SYSCALL_ALL {
        println!("handle_signal - handler: {:#x} ra: {:#x}", handler, ra);
    }
    // 使用handler的信号处理
    debug_assert!(![0, 1, usize::MAX].contains(&handler));
    let old_mask = mask;
    let mut new_mask = mask;
    new_mask.insert(sig_mask);
    new_mask.insert_bit(signal);
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
    uk_cx.set_signal_paramater(signal, 0, scx_ptr.as_usize());
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
            if PRINT_SYSCALL_ALL {
                println!("sigreturn restore mask: {:#x}", mask.0[0]);
            }
        }
    }
    Ok(thread.uk_context.a0())
}
