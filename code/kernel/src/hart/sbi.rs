#![allow(dead_code)]

use core::arch::asm;

#[inline(always)]
fn sbi_call<const N: usize>((fid, eid): (usize, usize), args: [usize; N]) -> usize {
    let ret: usize;
    unsafe {
        let mut a = [0; 3];
        a[..N].copy_from_slice(&args);
        asm!(
            "ecall",
            inlateout("a0") a[0] => ret,
            in("a1") a[1],
            in("a2") a[2],
            in("a6") fid,
            in("a7") eid,
        );
    }
    ret
}

pub fn console_putchar(c: usize) {
    sbi_call((0, SBI_CONSOLE_PUTCHAR), [c]);
}

pub fn console_getchar() -> usize {
    sbi_call((0, SBI_CONSOLE_GETCHAR), [])
}

pub fn shutdown() -> ! {
    sbi_call((0, SBI_SHUTDOWN), []);
    panic!("It should shutdown!");
}

pub fn set_timer(stime_value: u64) {
    #[cfg(target_pointer_width = "32")]
    sbi_call(
        (0, SBI_SET_TIMER),
        [stime_value as usize, (stime_value >> 32) as usize],
    );
    #[cfg(target_pointer_width = "64")]
    sbi_call((0, SBI_SET_TIMER), [stime_value as usize]);
}

pub fn clear_ipi() {
    sbi_call((0, SBI_CLEAR_IPI), []);
}

pub fn send_ipi(hart_mask: usize) {
    sbi_call((0, SBI_SEND_IPI), [&hart_mask as *const _ as usize]);
}

pub fn remote_fence_i(hart_mask: usize) {
    sbi_call((0, SBI_REMOTE_FENCE_I), [&hart_mask as *const _ as usize]);
}

pub fn remote_sfence_vma(hart_mask: usize, _start: usize, _size: usize) {
    sbi_call(
        (0, SBI_REMOTE_SFENCE_VMA),
        [&hart_mask as *const _ as usize],
    );
}

pub fn remote_sfence_vma_asid(hart_mask: usize, _start: usize, _size: usize, _asid: usize) {
    sbi_call(
        (0, SBI_REMOTE_SFENCE_VMA_ASID),
        [&hart_mask as *const _ as usize],
    );
}

pub fn sbi_hart_start(hartid: usize, start_addr: usize, opaque: usize) {
    sbi_call(SBI_HART_START, [hartid, start_addr, opaque]);
}

pub fn sbi_hart_get_status(hartid: usize) -> usize {
    sbi_call(SBI_HART_GET_STATUS, [hartid])
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
const SBI_HART_START: (usize, usize) = (0, 0x48534D);
const SBI_HART_STOP: (usize, usize) = (1, 0x48534D);
const SBI_HART_GET_STATUS: (usize, usize) = (2, 0x48534D);
const SBI_HART_GET_SUSPEND: (usize, usize) = (3, 0x48534D);
