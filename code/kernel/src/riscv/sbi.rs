#![allow(unused)]

use core::arch::asm;

#[inline(always)]
fn sbi_call<const N: usize>(sbi_id: usize, args: [usize; N]) -> usize {
    let ret: usize;
    unsafe {
        let x = core::mem::MaybeUninit::uninit().assume_init();
        let a = match *args.as_slice() {
            [] => (x, x, x),
            [a0] => (a0, x, x),
            [a0, a1] => (a0, a1, x),
            [a0, a1, a2] => (a0, a1, a2),
            _ => panic!(),
        };
        asm!(
            "ecall",
            inlateout("a0") a.0 => ret,
            in("a1") a.1,
            in("a2") a.2,
            in("a7") sbi_id
        );
    }
    ret
}

pub fn console_putchar(c: usize) {
    sbi_call(SBI_CONSOLE_PUTCHAR, [c]);
}

pub fn console_getchar() -> usize {
    sbi_call(SBI_CONSOLE_GETCHAR, [])
}

pub fn shutdown() -> ! {
    sbi_call(SBI_SHUTDOWN, []);
    panic!("It should shutdown!");
}

pub fn set_timer(stime_value: u64) {
    #[cfg(target_pointer_width = "32")]
    sbi_call(
        SBI_SET_TIMER,
        [stime_value as usize, (stime_value >> 32) as usize],
    );
    #[cfg(target_pointer_width = "64")]
    sbi_call(SBI_SET_TIMER, [stime_value as usize]);
}

pub fn clear_ipi() {
    sbi_call(SBI_CLEAR_IPI, []);
}

pub fn send_ipi(hart_mask: usize) {
    sbi_call(SBI_SEND_IPI, [&hart_mask as *const _ as usize]);
}

pub fn remote_fence_i(hart_mask: usize) {
    sbi_call(SBI_REMOTE_FENCE_I, [&hart_mask as *const _ as usize]);
}

pub fn remote_sfence_vma(hart_mask: usize, _start: usize, _size: usize) {
    sbi_call(SBI_REMOTE_SFENCE_VMA, [&hart_mask as *const _ as usize]);
}

pub fn remote_sfence_vma_asid(hart_mask: usize, _start: usize, _size: usize, _asid: usize) {
    sbi_call(
        SBI_REMOTE_SFENCE_VMA_ASID,
        [&hart_mask as *const _ as usize],
    );
}

const SBI_SET_TIMER: usize = 0;
const SBI_CONSOLE_PUTCHAR: usize = 1;
const SBI_CONSOLE_GETCHAR: usize = 2;
const SBI_CLEAR_IPI: usize = 3;
const SBI_SEND_IPI: usize = 4;
const SBI_REMOTE_FENCE_I: usize = 5;
const SBI_REMOTE_SFENCE_VMA: usize = 6;
const SBI_REMOTE_SFENCE_VMA_ASID: usize = 7;
const SBI_SHUTDOWN: usize = 8;
