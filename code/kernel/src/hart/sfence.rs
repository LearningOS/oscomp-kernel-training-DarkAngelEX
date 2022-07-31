#![allow(dead_code)]
use core::{arch::asm, mem::MaybeUninit};

use crate::memory::asid::USING_ASID;

#[inline(always)]
pub fn fence_i() {
    unsafe { asm!("fence.i") };
}

/// sfence_vma have two parameter, rs1 is address, rs2 is asid.
#[inline(always)]
pub fn sfence_vma_all_global() {
    unsafe {
        asm!("sfence.vma x0, x0");
    }
}

#[inline(always)]
pub fn sfence_vma_asid(asid: usize) {
    unsafe {
        asm!("sfence.vma x0, {}", in(reg)asid);
    }
}

#[inline(always)]
pub fn sfence_vma_all_no_global() {
    assert!(!USING_ASID);
    #[allow(unused_assignments)]
    unsafe {
        #[allow(clippy::uninit_assumed_init)]
        let mut x: usize = MaybeUninit::uninit().assume_init();
        asm!("mv {0}, zero",
            "sfence.vma x0, {0}",
            inout(reg)x
        );
    }
}

/// don't fflush global TLB.
#[inline(always)]
pub fn sfence_vma_va_global(va: usize) {
    unsafe {
        asm!(
        "sfence.vma {0}, x0", in(reg)va
        );
    }
}

/// fflush all TLB in this va but not global TLB
#[inline(always)]
pub fn sfence_vma_va(va: usize) {
    sfence_vma_va_asid(va, 0);
}

/// don't fflush global TLB.
#[inline(always)]
pub fn sfence_vma_va_asid(va: usize, asid: usize) {
    #[allow(unused_assignments)]
    unsafe {
        #[allow(clippy::uninit_assumed_init)]
        let mut x: usize = MaybeUninit::uninit().assume_init();
        asm!(
        "add {2}, x0, {1}",
        "sfence.vma {0}, {2}",
        in(reg)va,in(reg)asid, inout(reg) x
        );
    }
}
