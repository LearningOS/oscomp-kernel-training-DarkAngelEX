#![allow(dead_code, unused_macros)]

use core::{arch::asm, mem::MaybeUninit};

pub unsafe fn set_satp(satp: usize) {
    asm!("csrw satp, {}", in(reg)satp);
}
pub unsafe fn get_satp() -> usize {
    let ret;
    asm!("csrr {}, satp", out(reg)ret);
    ret
}

pub fn get_sp() -> usize {
    let ret;
    unsafe {
        asm!("mv {}, sp", out(reg)ret);
    }
    ret
}
///
/// sfence_vma have two parameter, rs1 is address, rs2 is asid.
///
pub fn sfence_vma_all_global() {
    unsafe {
        asm!("sfence.vma x0, x0");
    }
}
#[allow(unused_assignments)]
pub fn sfence_vma_all_no_global() {
    unsafe {
        // alloc a register, assume rs2 != x0
        let mut x: usize = MaybeUninit::uninit().assume_init();
        asm!(
        "add {0}, x0, x0",
        "sfence.vma x0, {0}",
        inout(reg) x
        );
    }
}
pub fn sfence_vma_asid(asid: usize) {
    unsafe {
        asm!("sfence.vma x0, {}", in(reg)asid);
    }
}
///
/// no fflush global TLB.
///
pub fn sfence_vma_va_global(va: usize) {
    unsafe {
        asm!(
        "sfence.vma {0}, x0", in(reg)va
        );
    }
}
///
/// fflush all TLB in this va but not global TLB
///
pub fn sfence_vma_va(va: usize) {
    sfence_vma_va_asid(va, 0);
}
///
/// no fflush global TLB.
///
#[allow(unused_assignments)]
pub fn sfence_vma_va_asid(va: usize, asid: usize) {
    unsafe {
        // alloc a register, assume rs2 != x0
        let mut x: usize = MaybeUninit::uninit().assume_init();
        asm!(
        "add {2}, x0, {1}",
        "sfence.vma {0}, {2}",
        in(reg)va,in(reg)asid, inout(reg) x
        );
    }
}

macro_rules! read_csr {
    ($csr_number:expr, $asm_fn: ident) => {
        /// Reads the CSR
        #[inline]
        unsafe fn _read() -> usize {
            let r: usize;
            core::arch::asm!("csrrs {0}, {1}, x0", out(reg) r, const $csr_number);
            r
        }
    };
}

macro_rules! read_csr_as {
    ($register:ident, $csr_number:expr, $asm_fn: ident) => {
        read_csr!($csr_number, $asm_fn);

        /// Reads the CSR
        #[inline]
        pub fn read() -> $register {
            $register {
                bits: unsafe { _read() },
            }
        }
    };
}
macro_rules! read_csr_as_usize {
    ($csr_number:expr, $asm_fn: ident) => {
        read_csr!($csr_number, $asm_fn);

        /// Reads the CSR
        #[inline]
        pub fn read() -> usize {
            unsafe { _read() }
        }
    };
}

macro_rules! write_csr {
    ($csr_number:expr, $asm_fn: ident) => {
        /// Writes the CSR
        #[inline]
        #[allow(unused_variables)]
        unsafe fn _write(bits: usize) {
            core::arch::asm!("csrrw x0, {1}, {0}", in(reg) bits, const $csr_number)
        }
    };
}

macro_rules! write_csr_as_usize {
    ($csr_number:expr, $asm_fn: ident) => {
        write_csr!($csr_number, $asm_fn);

        /// Writes the CSR
        #[inline]
        pub fn write(bits: usize) {
            unsafe { _write(bits) }
        }
    };
}

macro_rules! set {
    ($csr_number:expr, $asm_fn: ident) => {
        /// Set the CSR
        #[inline]
        #[allow(unused_variables)]
        unsafe fn _set(bits: usize) {
            core::arch::asm!("csrrs x0, {1}, {0}", in(reg) bits, const $csr_number)
        }
    };
}

macro_rules! clear {
    ($csr_number:expr, $asm_fn: ident) => {
        /// Clear the CSR
        #[inline]
        #[allow(unused_variables)]
        unsafe fn _clear(bits: usize) {
            core::arch::asm!("csrrc x0, {1}, {0}", in(reg) bits, const $csr_number)
        }
    };
}

macro_rules! set_csr {
    ($(#[$attr:meta])*, $set_field:ident, $e:expr) => {
        $(#[$attr])*
        #[inline]
        pub unsafe fn $set_field() {
            _set($e);
        }
    };
}

macro_rules! clear_csr {
    ($(#[$attr:meta])*, $clear_field:ident, $e:expr) => {
        $(#[$attr])*
        #[inline]
        pub unsafe fn $clear_field() {
            _clear($e);
        }
    };
}

macro_rules! set_clear_csr {
    ($(#[$attr:meta])*, $set_field:ident, $clear_field:ident, $e:expr) => {
        set_csr!($(#[$attr])*, $set_field, $e);
        clear_csr!($(#[$attr])*, $clear_field, $e);
    }
}

macro_rules! read_composite_csr {
    ($hi:expr, $lo:expr) => {
        /// Reads the CSR as a 64-bit value
        #[inline]
        pub fn read64() -> u64 {
            match () {
                #[cfg(riscv32)]
                () => loop {
                    let hi = $hi;
                    let lo = $lo;
                    if hi == $hi {
                        return ((hi as u64) << 32) | lo as u64;
                    }
                },

                #[cfg(not(riscv32))]
                () => $lo as u64,
            }
        }
    };
}
