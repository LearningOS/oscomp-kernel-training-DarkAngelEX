#![allow(dead_code)]
use core::{arch::asm, mem::MaybeUninit};

#[inline(always)]
pub fn fence_i() {
    unsafe { asm!("fence.i") };
}
///
/// sfence_vma have two parameter, rs1 is address, rs2 is asid.
///
#[inline(always)]
pub fn sfence_vma_all_global() {
    unsafe {
        asm!("sfence.vma x0, x0");
    }
}

#[inline(always)]
#[allow(unused_assignments)]
pub fn sfence_vma_asid_zero() {
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

#[inline(always)]
pub fn sfence_vma_asid(asid: usize) {
    unsafe {
        asm!("sfence.vma x0, {}", in(reg)asid);
    }
}
///
/// no fflush global TLB.
///
#[inline(always)]
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
#[inline(always)]
pub fn sfence_vma_va(va: usize) {
    sfence_vma_va_asid(va, 0);
}
///
/// no fflush global TLB.
///
#[inline(always)]
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
