use core::mem::MaybeUninit;

use crate::memory::address::{UserAddr, UserAddr4K};
use crate::memory::user_ptr::{Policy, UserPtr};
use crate::riscv::register::sstatus::Sstatus;
use crate::user::SpaceGuard;

use super::run_user;

#[repr(C)]
pub struct UKContext {
    pub user_rx: [usize; 32],   // 0-31
    pub user_sepc: UserAddr,    // 32
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

usize_forward_impl!(usize, a, a);
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

impl<T, P: Policy> UsizeForward for UserPtr<T, P> {
    fn usize_forward(a: usize) -> Self {
        Self::from_usize(a)
    }
}

impl UKContext {
    pub unsafe fn any() -> Self {
        MaybeUninit::uninit().assume_init()
        // MaybeUninit::zeroed().assume_init()
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
    pub fn set_argc_argv(&mut self, argc: usize, argv: usize) {
        self.user_rx[10] = argc;
        self.user_rx[11] = argv;
    }
    pub fn parameter1<T: UsizeForward>(&self) -> T {
        T::usize_forward(self.user_rx[10])
    }
    pub fn parameter2<A: UsizeForward, B: UsizeForward>(&self) -> (A, B) {
        (
            A::usize_forward(self.user_rx[10]),
            B::usize_forward(self.user_rx[11]),
        )
    }
    pub fn parameter3<A: UsizeForward, B: UsizeForward, C: UsizeForward>(&self) -> (A, B, C) {
        (
            A::usize_forward(self.user_rx[10]),
            B::usize_forward(self.user_rx[11]),
            C::usize_forward(self.user_rx[12]),
        )
    }
    // pub fn syscall_parameter<const N: usize>(&self) -> &[usize; N] {
    //     let rx = &self.user_rx;
    //     rx.rsplit_array_ref::<22>().1.split_array_ref().0
    // }
    /// sepc += 4
    pub fn into_next_instruction(&mut self) {
        self.user_sepc.add_assign(4);
    }

    pub fn exec_init(
        &mut self,
        user_sp: UserAddr,
        sepc: UserAddr,
        sstatus: Sstatus,
        argc: usize,
        argv: usize,
    ) {
        self.set_user_sp(user_sp.into_usize());
        self.set_argc_argv(argc, argv);
        self.user_sepc = sepc;
        self.user_sstatus = sstatus;
    }

    pub fn fork(&self) -> Self {
        let mut new = unsafe { Self::any() };
        new.user_rx = self.user_rx;
        new.user_sepc = self.user_sepc;
        new.user_sstatus = self.user_sstatus;
        new
    }
    pub fn run_user(&mut self, _mark: &SpaceGuard) {
        run_user(self)
    }
}
