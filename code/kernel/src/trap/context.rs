use alloc::sync::Arc;
use ftl_util::fs::Mode;

use riscv::register::{fcsr::FCSR, scause::Scause, sstatus::FS};

use crate::{
    hart::floating::{self, FLOAT_ENABLE},
    memory::{
        address::UserAddr,
        user_ptr::{Policy, UserPtr},
    },
    process::{thread::Thread, Process},
    riscv::register::sstatus::Sstatus,
    signal::Sig,
    tools::allocator::from_usize_allocator::FromUsize,
};

use super::FastStatus;

pub struct FastContext {
    pub thread: &'static Thread,
    pub thread_arc: &'static Arc<Thread>,
    pub process: &'static Process,
    pub skip_syscall: bool,
}

impl FastContext {
    pub unsafe fn new(thread: &Thread, thread_arc: &Arc<Thread>, process: &Process) -> Self {
        Self {
            thread: &*(thread as *const _),
            thread_arc: &*(thread_arc as *const _),
            process: &*(process as *const _),
            skip_syscall: false,
        }
    }
}

/// user-kernel context
#[repr(C)]
pub struct UKContext {
    pub user_rx: [usize; 32],   // 0-31, sepc is [0]
    pub user_sepc: usize,       // 32
    pub user_sstatus: Sstatus,  // 33
    pub kernel_sx: [usize; 12], // 34-45
    pub kernel_ra: usize,       // 46
    pub kernel_sp: usize,       // 47
    pub kernel_gp: usize,       // 48
    pub kernel_tp: usize,       // 49
    pub scause: Scause,         // 50
    pub stval: usize,           // 51
    pub user_fx: FloatContext,
    // 快速处理路径中转
    pub fast_context: usize, // 指向 FastContext
    pub to_executor: FastStatus,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FloatContext {
    pub fx: [f64; 32],
    pub fcsr: FCSR, // 32bit
    // because of repr(C), use u8 instead of bool
    pub need_save: u8, // become 1 when dirty, run save when switch context
    pub need_load: u8, // become 1 when switch context, run load when into user
    pub sig_dirty: u8, // become 1 when dirty, clean when signal return
}

impl FloatContext {
    pub fn reset(&mut self, fcsr: FCSR) {
        self.fx.fill(0.);
        self.fcsr = fcsr;
        self.need_load = 1;
        self.need_save = 0;
        self.sig_dirty = 0;
    }
}

pub trait UsizeForward {
    fn usize_forward(a: usize) -> Self;
}
macro_rules! usize_forward_impl {
    ($type: ty, $var: ident, $body: expr) => {
        impl UsizeForward for $type {
            fn usize_forward($var: usize) -> $type {
                $body
            }
        }
    };
    ($type: ty, $var: ident, $body: expr, $T: tt) => {
        impl<$T> UsizeForward for $type {
            fn usize_forward($var: usize) -> $type {
                $body
            }
        }
    };
}

// usize_forward_impl!(usize, a, a);
usize_forward_impl!(isize, a, a as isize);
usize_forward_impl!(u32, a, a as u32);
usize_forward_impl!(i32, a, a as i32);
usize_forward_impl!(u16, a, a as u16);
usize_forward_impl!(i16, a, a as i16);
usize_forward_impl!(u8, a, a as u8);
usize_forward_impl!(i8, a, a as i8);
usize_forward_impl!(Mode, a, Mode(a as u32));
usize_forward_impl!(*const T, a, a as *const T, T);
usize_forward_impl!(*mut T, a, a as *mut T, T);

impl<T: Clone + Copy + 'static, P: Policy> UsizeForward for UserPtr<T, P> {
    #[inline(always)]
    fn usize_forward(a: usize) -> Self {
        Self::from_usize(a)
    }
}
impl<T: FromUsize> UsizeForward for T {
    #[inline(always)]
    fn usize_forward(a: usize) -> Self {
        T::from_usize(a)
    }
}

macro_rules! para_impl {
    ($fn_name: ident, $T:tt) => {
        #[inline(always)]
        pub fn $fn_name<$T: UsizeForward>(&self) -> $T {
            $T::usize_forward(self.user_rx[10])
        }
    };
    ($fn_name: ident, $($T:tt),*) => {
        #[allow(dead_code)]
        #[inline(always)]
        pub fn $fn_name<$($T: UsizeForward,)*>(&self) -> ($($T,)*)
        {
            let mut i = 0..;
            ($($T::usize_forward(self.user_rx[10 + i.next().unwrap()]),)*)
        }
    };
}

macro_rules! cx_into_impl {
    ($src: ty, $($T:tt),*) => {
        impl<$($T: UsizeForward,)*> From<$src> for ($($T,)*) {
            fn from(cx: $src) -> Self {
                let mut i = 0..;
                ($($T::usize_forward(cx.user_rx[10 + i.next().unwrap()]),)*)
            }
        }
    };
}

cx_into_impl!(&UKContext, A, B);
cx_into_impl!(&UKContext, A, B, C);
cx_into_impl!(&UKContext, A, B, C, D);
cx_into_impl!(&UKContext, A, B, C, D, E);
cx_into_impl!(&UKContext, A, B, C, D, E, F);
cx_into_impl!(&UKContext, A, B, C, D, E, F, G);
cx_into_impl!(&mut UKContext, A, B);
cx_into_impl!(&mut UKContext, A, B, C);
cx_into_impl!(&mut UKContext, A, B, C, D);
cx_into_impl!(&mut UKContext, A, B, C, D, E);
cx_into_impl!(&mut UKContext, A, B, C, D, E, F);
cx_into_impl!(&mut UKContext, A, B, C, D, E, F, G);

impl Default for UKContext {
    fn default() -> Self {
        Self::new()
    }
}

impl UKContext {
    pub fn new() -> Self {
        unsafe { core::mem::zeroed() }
    }
    pub fn set_fast_context(&mut self, fc: &FastContext) {
        self.fast_context = fc as *const _ as usize;
    }
    pub fn fast_context(&self) -> &'static FastContext {
        unsafe { &*(self.fast_context as *const FastContext) }
    }
    #[inline(always)]
    pub fn a0(&self) -> usize {
        self.user_rx[10]
    }
    #[inline(always)]
    pub fn a7(&self) -> usize {
        self.user_rx[17]
    }
    #[inline(always)]
    pub fn a0_a7(&self) -> &[usize] {
        &self.user_rx[10..=17]
    }
    #[inline(always)]
    pub fn ra(&self) -> usize {
        self.user_rx[1]
    }
    #[inline(always)]
    pub fn sp(&self) -> usize {
        self.user_rx[2]
    }
    #[inline(always)]
    pub fn set_user_sepc(&mut self, sepc: usize) {
        self.user_sepc = sepc;
    }
    #[inline(always)]
    pub fn set_user_ra(&mut self, ra: usize) {
        self.user_rx[1] = ra;
    }
    #[inline(always)]
    pub fn set_user_sp(&mut self, sp: usize) {
        self.user_rx[2] = sp;
    }
    #[inline(always)]
    pub fn set_user_tp(&mut self, tp: usize) {
        self.user_rx[4] = tp;
    }
    #[inline(always)]
    pub fn set_user_a0(&mut self, a0: usize) {
        self.user_rx[10] = a0;
    }
    #[inline(always)]
    pub fn set_signal_paramater(&mut self, sig: Sig, si: usize, ctx: usize) {
        self.user_rx[10..=12].copy_from_slice(&[sig.to_user() as usize, si, ctx]);
    }
    #[inline(always)]
    pub fn set_argc_argv_envp(&mut self, argc: usize, argv: usize, envp: usize) {
        self.user_rx[10] = argc;
        self.user_rx[11] = argv;
        self.user_rx[12] = envp;
    }
    para_impl!(para1, A);
    para_impl!(para2, A, B);
    para_impl!(para3, A, B, C);
    para_impl!(para4, A, B, C, D);
    para_impl!(para5, A, B, C, D, E);
    para_impl!(para6, A, B, C, D, E, F);
    para_impl!(para7, A, B, C, D, E, F, G);

    // pub fn syscall_parameter<const N: usize>(&self) -> &[usize; N] {
    //     let rx = &self.user_rx;
    //     rx.rsplit_array_ref::<22>().1.split_array_ref().0
    // }
    /// sepc += 4
    #[inline(always)]
    pub fn set_next_instruction(&mut self) {
        self.user_sepc += 4;
    }

    pub fn exec_init(
        &mut self,
        user_sp: UserAddr<u8>,
        sepc: UserAddr<u8>,
        sstatus: Sstatus,
        fcsr: FCSR,
        (argc, argv, envp): (usize, usize, usize),
    ) {
        self.user_rx.fill(0);
        self.user_fx.reset(fcsr);
        self.set_user_sp(user_sp.into_usize());
        self.set_argc_argv_envp(argc, argv, envp);
        self.user_sepc = sepc.into_usize();
        self.user_sstatus = sstatus;
    }

    pub fn fork(&self, tls: Option<usize>) -> Self {
        let mut new = Self::new();
        new.user_rx = self.user_rx;
        if let Some(tp) = tls {
            new.set_user_tp(tp)
        }
        new.user_sepc = self.user_sepc;
        new.user_sstatus = self.user_sstatus;
        if FLOAT_ENABLE {
            new.user_fx = self.user_fx;
            new.user_fx.need_load = 1;
        }
        new
    }
    /// 由执行器调用, 进入用户态
    pub fn run_user_executor(&mut self) {
        debug_assert!(!self.user_sstatus.sie());
        if FLOAT_ENABLE {
            unsafe { floating::load_fx(&mut self.user_fx) };
            self.user_sstatus.set_fs(FS::Clean);
        }
        super::run_user_executor(self);
        if FLOAT_ENABLE {
            floating::store_fx_mark(&mut self.user_fx, &mut self.user_sstatus);
        }
    }
}
