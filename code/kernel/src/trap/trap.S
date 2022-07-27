    .section .text
    .global __entry_user
    .global __return_from_user
    .global __kernel_default_exception_entry
    .global __kernel_default_interrupt_entry
    .global __kernel_default_vector
    .global __kernel_default_trap_entry
    # .global __load_fx
    # .global __store_fx
.altmacro
// a0(user_context)
    .align 6
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
    .align 6
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

    .align 6
// using current stack
__kernel_default_exception_entry:
    addi sp, sp, -17*8
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
    sd  a7, 16*8(sp)
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
    ld  a7, 16*8(sp)
    addi sp, sp, 17*8
    sret

    .align 6
__kernel_default_interrupt_entry:
    addi sp, sp, -17*8
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
    sd  a7, 16*8(sp)
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
    ld  a7, 16*8(sp)
    addi sp, sp, 17*8
    sret

    .align 8
__kernel_default_vector:
    j __kernel_default_exception_entry
    .rept 16
    .align 2
    j __kernel_default_interrupt_entry
    .endr

    .align 6
__kernel_default_trap_entry:
    addi sp, sp, -17*8
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
    sd  a7, 16*8(sp)
    mv  a0, sp
    call kernel_default_trap
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
    ld  a7, 16*8(sp)
    addi sp, sp, 17*8
    sret

# # memory -> register
# __load_fx:
#     fld  f0,  0*8(a0)
#     fld  f1,  1*8(a0)
#     fld  f2,  2*8(a0)
#     fld  f3,  3*8(a0)
#     fld  f4,  4*8(a0)
#     fld  f5,  5*8(a0)
#     fld  f6,  6*8(a0)
#     fld  f7,  7*8(a0)
#     fld  f8,  8*8(a0)
#     fld  f9,  9*8(a0)
#     fld f10, 10*8(a0)
#     fld f11, 11*8(a0)
#     fld f12, 12*8(a0)
#     fld f13, 13*8(a0)
#     fld f14, 14*8(a0)
#     fld f15, 15*8(a0)
#     fld f16, 16*8(a0)
#     fld f17, 17*8(a0)
#     fld f18, 18*8(a0)
#     fld f19, 19*8(a0)
#     fld f20, 20*8(a0)
#     fld f21, 21*8(a0)
#     fld f22, 22*8(a0)
#     fld f23, 23*8(a0)
#     fld f24, 24*8(a0)
#     fld f25, 25*8(a0)
#     fld f26, 26*8(a0)
#     fld f27, 27*8(a0)
#     fld f28, 28*8(a0)
#     fld f29, 29*8(a0)
#     fld f30, 30*8(a0)
#     fld f31, 31*8(a0)
#     lw  a0,  32*8(a0)
#     csrw fcsr, a0
#     ret

# # register -> memory
# __store_fx:
#     fsd  f0,  0*8(a0)
#     fsd  f1,  1*8(a0)
#     fsd  f2,  2*8(a0)
#     fsd  f3,  3*8(a0)
#     fsd  f4,  4*8(a0)
#     fsd  f5,  5*8(a0)
#     fsd  f6,  6*8(a0)
#     fsd  f7,  7*8(a0)
#     fsd  f8,  8*8(a0)
#     fsd  f9,  9*8(a0)
#     fsd f10, 10*8(a0)
#     fsd f11, 11*8(a0)
#     fsd f12, 12*8(a0)
#     fsd f13, 13*8(a0)
#     fsd f14, 14*8(a0)
#     fsd f15, 15*8(a0)
#     fsd f16, 16*8(a0)
#     fsd f17, 17*8(a0)
#     fsd f18, 18*8(a0)
#     fsd f19, 19*8(a0)
#     fsd f20, 20*8(a0)
#     fsd f21, 21*8(a0)
#     fsd f22, 22*8(a0)
#     fsd f23, 23*8(a0)
#     fsd f24, 24*8(a0)
#     fsd f25, 25*8(a0)
#     fsd f26, 26*8(a0)
#     fsd f27, 27*8(a0)
#     fsd f28, 28*8(a0)
#     fsd f29, 29*8(a0)
#     fsd f30, 30*8(a0)
#     fsd f31, 31*8(a0)
#     csrr a0, fcsr
#     sw  a0,  32*8(a0)
#     ret
