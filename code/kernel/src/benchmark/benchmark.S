    .section .text
    .global __kernel_benchmark_exception_entry
    .global __kernel_benchmark_exception_save_entry
    .align 2
__kernel_benchmark_exception_entry:
    addi sp, sp, -8
    sd   a0, 0(sp)
    csrr a0, sepc
    addi a0, a0, 4
    csrw sepc, a0
    ld   a0, 0(sp)
    addi sp, sp, 8
    sret

    .align 2
// using current stack
__kernel_benchmark_exception_save_entry:
    addi sp, sp, -2*8
    sd  a7,  0(sp)
    # # disable interrupt(sie)
    csrrci a7, sstatus, 2
    sd  a7,  8(sp)
    addi sp, sp, -16*8
    sd  ra,  1*8(sp)
    sd  t0,  2*8(sp)
    sd  t1,  3*8(sp)
    sd  t2,  4*8(sp)
    sd  t3,  5*8(sp)
    sd  t4,  6*8(sp)
    sd  t5,  7*8(sp)
    sd  t6,  8*8(sp)
    sd  a0,  9*8(sp)
    sd  a1, 10*8(sp)
    sd  a2, 11*8(sp)
    sd  a3, 12*8(sp)
    sd  a4, 13*8(sp)
    sd  a5, 14*8(sp)
    sd  a6, 15*8(sp)
    # call kernel_default_exception
    addi sp, sp, -8
    sd   a0, 0(sp)
    csrr a0, sepc
    addi a0, a0, 4
    csrw sepc, a0
    ld   a0, 0(sp)
    addi sp, sp, 8

    ld  ra,  1*8(sp)
    ld  t0,  2*8(sp)
    ld  t1,  3*8(sp)
    ld  t2,  4*8(sp)
    ld  t3,  5*8(sp)
    ld  t4,  6*8(sp)
    ld  t5,  7*8(sp)
    ld  t6,  8*8(sp)
    ld  a0,  9*8(sp)
    ld  a1, 10*8(sp)
    ld  a2, 11*8(sp)
    ld  a3, 12*8(sp)
    ld  a4, 13*8(sp)
    ld  a5, 14*8(sp)
    ld  a6, 15*8(sp)
    # ld  a7, 16*8(sp)
    addi sp, sp, 16*8
    # enable interrupt. do like this can use just 1 Byte each interrupt.
    ld  a7, 8(sp)
    csrs sstatus, a7
    ld  a7, 0(sp)
    addi sp, sp, 2*8
    sret
