    .section .text
    .global __entry_user
    .global __return_from_user
    .global __kernel_default_exception_entry
    .global __kernel_default_interrupt_entry
    .global __kernel_default_vector
.altmacro
// a0(user_context)
    .align 2
__entry_user:
    sd  s0, 34*8(a0)
    sd  s1, 35*8(a0)
    sd  s2, 36*8(a0)
    sd  s3, 37*8(a0)
    sd  s4, 38*8(a0)
    sd  s5, 39*8(a0)
    sd  s6, 40*8(a0)
    sd  s7, 41*8(a0)
    sd  s8, 42*8(a0)
    sd  s9, 43*8(a0)
    sd s10, 44*8(a0)
    sd s11, 45*8(a0)
    sd  ra, 46*8(a0)
    sd  sp, 47*8(a0)
    sd  tp, 48*8(a0)
    ld  t0, 32*8(a0)
    ld  t1, 33*8(a0)
    csrw sscratch,a0
    csrw sepc, t0
    csrw sstatus, t1
    ld  ra,  1*8(a0)
    ld  sp,  2*8(a0)
    ld  gp,  3*8(a0)
    ld  tp,  4*8(a0)
    ld  t0,  5*8(a0)
    ld  t1,  6*8(a0)
    ld  t2,  7*8(a0)
    ld  s0,  8*8(a0)
    ld  s1,  9*8(a0)
    // skip a0
    ld  a1, 11*8(a0)
    ld  a2, 12*8(a0)
    ld  a3, 13*8(a0)
    ld  a4, 14*8(a0)
    ld  a5, 15*8(a0)
    ld  a6, 16*8(a0)
    ld  a7, 17*8(a0)
    ld  s2, 18*8(a0)
    ld  s3, 19*8(a0)
    ld  s4, 20*8(a0)
    ld  s5, 21*8(a0)
    ld  s6, 22*8(a0)
    ld  s7, 23*8(a0)
    ld  s8, 24*8(a0)
    ld  s9, 25*8(a0)
    ld s10, 26*8(a0)
    ld s11, 27*8(a0)
    ld  t3, 28*8(a0)
    ld  t4, 29*8(a0)
    ld  t5, 30*8(a0)
    ld  t6, 31*8(a0)
    ld  a0, 10*8(a0) // load a0
    sret
    // (running in user...)
__return_from_user:
    csrrw a0, sscratch, a0
    sd  ra,  1*8(a0)
    sd  sp,  2*8(a0)
    sd  gp,  3*8(a0)
    sd  tp,  4*8(a0)
    sd  t0,  5*8(a0)
    sd  t1,  6*8(a0)
    sd  t2,  7*8(a0)
    sd  s0,  8*8(a0)
    sd  s1,  9*8(a0)
    csrr t0, sscratch // a0
    sd  t0, 10*8(a0)
    sd  a1, 11*8(a0)
    sd  a2, 12*8(a0)
    sd  a3, 13*8(a0)
    sd  a4, 14*8(a0)
    sd  a5, 15*8(a0)
    sd  a6, 16*8(a0)
    sd  a7, 17*8(a0)
    sd  s2, 18*8(a0)
    sd  s3, 19*8(a0)
    sd  s4, 20*8(a0)
    sd  s5, 21*8(a0)
    sd  s6, 22*8(a0)
    sd  s7, 23*8(a0)
    sd  s8, 24*8(a0)
    sd  s9, 25*8(a0)
    sd s10, 26*8(a0)
    sd s11, 27*8(a0)
    sd  t3, 28*8(a0)
    sd  t4, 29*8(a0)
    sd  t5, 30*8(a0)
    sd  t6, 31*8(a0)
    csrr t0, sepc
    csrr t1, sstatus
    sd  t0, 32*8(a0)
    sd  t1, 33*8(a0)
    ld  s0, 34*8(a0)
    ld  s1, 35*8(a0)
    ld  s2, 36*8(a0)
    ld  s3, 37*8(a0)
    ld  s4, 38*8(a0)
    ld  s5, 39*8(a0)
    ld  s6, 40*8(a0)
    ld  s7, 41*8(a0)
    ld  s8, 42*8(a0)
    ld  s9, 43*8(a0)
    ld s10, 44*8(a0)
    ld s11, 45*8(a0)
    ld  ra, 46*8(a0)
    ld  sp, 47*8(a0)
    ld  tp, 48*8(a0)
    ret

    .align 2
// using current stack
__kernel_default_exception_entry:
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
    call kernel_default_exception
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

    .align 2
__kernel_default_interrupt_entry:
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
    # sd  a7, 16*8(sp)
    call kernel_default_interrupt
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

__kernel_default_vector:
    .align 2
    j __kernel_default_exception_entry
    .rept 16
    .align 2
    j __kernel_default_interrupt_entry
    .endr
