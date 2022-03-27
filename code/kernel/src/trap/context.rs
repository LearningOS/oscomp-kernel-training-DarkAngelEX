use alloc::boxed::Box;

use crate::memory::{
    address::UserAddr,
    user_ptr::{Policy, UserPtr},
};
use crate::riscv::register::sstatus::Sstatus;
use crate::tools::allocator::from_usize_allocator::FromUsize;

#[repr(C)]
pub struct UKContext {
    pub user_rx: [usize; 32],   // 0-31
    pub user_sepc: usize,       // 32
    pub user_sstatus: Sstatus,  // 33
    pub kernel_sx: [usize; 12], // 34-45
    pub kernel_ra: usize,       // 46
    pub kernel_sp: usize,       // 47
    pub kernel_tp: usize,       // 48
}

pub trait UsizeForward {
    fn usize_forward(a: usize) -> Self;
}
macro_rules! usize_forward_impl {
    ($type: ident, $var: ident, $body: expr) => {
        impl UsizeForward for $type {
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
impl<T> UsizeForward for *const T {
    fn usize_forward(a: usize) -> *const T {
        a as *const T
    }
}
impl<T> UsizeForward for *mut T {
    fn usize_forward(a: usize) -> *mut T {
        a as *mut T
    }
}

impl<T: Clone + Copy + 'static, P: Policy> UsizeForward for UserPtr<T, P> {
    fn usize_forward(a: usize) -> Self {
        Self::from_usize(a)
    }
}
impl<T: FromUsize> UsizeForward for T {
    fn usize_forward(a: usize) -> Self {
        T::from_usize(a)
    }
}

macro_rules! para_impl {
    ($fn_name: ident, $T:tt) => {
        pub fn $fn_name<$T: UsizeForward>(&self) -> $T {
            $T::usize_forward(self.user_rx[10])
        }
    };
    ($fn_name: ident, $($T:tt),*) => {
        #[allow(dead_code)]
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

impl UKContext {
    pub unsafe fn any() -> Box<Self> {
        Box::new_uninit().assume_init()
    }
    pub fn a7(&self) -> usize {
        self.user_rx[17]
    }
    pub fn set_user_sp(&mut self, sp: usize) {
        self.user_rx[2] = sp;
    }
    pub fn set_user_a0(&mut self, a0: usize) {
        self.user_rx[10] = a0;
    }
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
    pub fn set_next_instruction(&mut self) {
        self.user_sepc += 4;
    }

    pub fn exec_init(
        &mut self,
        user_sp: UserAddr,
        sepc: UserAddr,
        sstatus: Sstatus,
        argc: usize,
        argv: usize,
        envp: usize,
    ) {
        self.set_user_sp(user_sp.into_usize());
        self.set_argc_argv_envp(argc, argv, envp);
        self.user_sepc = sepc.into_usize();
        self.user_sstatus = sstatus;
    }

    pub fn fork(&self) -> Box<Self> {
        let mut new = unsafe { Self::any() };
        new.user_rx = self.user_rx;
        new.user_sepc = self.user_sepc;
        new.user_sstatus = self.user_sstatus;
        new
    }
    pub fn run_user(&mut self) {
        super::run_user(self)
    }
}
