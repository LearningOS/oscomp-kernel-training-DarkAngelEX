# .altmacro
# .macro print ch
#     mv s11, a7
#     mv s10, a0
#     li a7, 1
#     li a0, \ch
#     ecall
#     mv a7, s11
#     mv a0, s10
# .endm
    .section .text.trampoline
    .global __trap_entry
    .global __trap_return
    .align 2
// fast_syscall:
// a0 <= sscratch <= a0
// save sp
// sp <= scause
// beq sp, 8 => fast_syscall_entry
// trap_handler_entry:
// save ra, gp, tp
// save a1-a7 t0-t6 (ignore s0-s11)
// save sscratch(user a0) by t2
// save sstatus by t0
// save sepc by t1
// load kernel_sp
// call trap_handler(a0: frame) -> (a0: frame)
// sscratch <= a0
// load sstatus, sepc by a0
// load ra, sp, gp, tp
// load a1-a7 t0-t6 (ignore s0-s11)
// load a0
// sret
//
// fast_syscall_entry:
// save ra, gp, tp
// save sscratch(user a0) by t2
// save sstatus by t0
// save sepc by t1
// load kernel_sp
// mv a6, a0
// call fast_syscall -> (a0, a1)
// sscratch <= a1
// load sstatus, sepc by a1
// load ra, sp, gp, tp
// sret (with a0 as return value)
//

__trap_entry:
    // 0(a0)-31*8(a0) x0-x31 a0 is x10
    // 32*8(a0) sstatus
    // 33*8(a0) sepc
    // 34*8(a0) kernel_sp
    csrrw a0, sscratch, a0
    sd   sp,   2*8(a0)
    csrr sp,  scause
    addi sp,  sp, -8
    beqz sp,  fast_syscall_entry
trap_handler_entry:
    # x1-x4
    sd  ra,   1*8(a0)
    sd  gp,   3*8(a0)
    sd  tp,   4*8(a0)
    # x5-x9
    sd  t0,   5*8(a0)
    sd  t1,   6*8(a0)
    sd  t2,   7*8(a0)
    # sd  s0,   8*8(a0)
    # sd  s1,   9*8(a0)
    // skip x10/a0
    # x11-x17
    sd  a1, 11*8(a0)
    sd  a2, 12*8(a0)
    sd  a3, 13*8(a0)
    sd  a4, 14*8(a0)
    sd  a5, 15*8(a0)
    sd  a6, 16*8(a0)
    sd  a7, 17*8(a0)
    # x18-x27
    # sd  s2, 18*8(a0)
    # sd  s3, 19*8(a0)
    # sd  s4, 20*8(a0)
    # sd  s5, 21*8(a0)
    # sd  s6, 22*8(a0)
    # sd  s7, 23*8(a0)
    # sd  s8, 24*8(a0)
    # sd  s9, 25*8(a0)
    # sd s10, 26*8(a0)
    # sd s11, 27*8(a0)
    # x27-x31
    sd  t3, 28*8(a0)
    sd  t4, 29*8(a0)
    sd  t5, 30*8(a0)
    sd  t6, 31*8(a0)
    // store a0
    csrr t2, sscratch
    sd   t2, 10*8(a0)
    csrr t0, sstatus
    sd   t0, 32*8(a0)
    csrr t1, sepc
    sd  t1, 33*8(a0)
    // set kernel_sp
    ld  sp, 34*8(a0)
    call trap_handler // -> a0: frame ptr
__trap_return:

    // return from same place.
    csrw sscratch, a0
    ld t0, 32*8(a0)
    csrw sstatus, t0
    ld t1, 33*8(a0)
    csrw sepc, t1       # user entry
    # x1-x4
    ld  ra,   1*8(a0)
    ld  sp,   2*8(a0)
    ld  gp,   3*8(a0)
    ld  tp,   4*8(a0)
    # x5-x9
    ld  t0,   5*8(a0)
    ld  t1,   6*8(a0)
    ld  t2,   7*8(a0)
    # sd  s0,   8*8(a0)
    # sd  s1,   9*8(a0)
    // skip x10/a0
    # x11-x17
    ld  a1, 11*8(a0)
    ld  a2, 12*8(a0)
    ld  a3, 13*8(a0)
    ld  a4, 14*8(a0)
    ld  a5, 15*8(a0)
    ld  a6, 16*8(a0)
    ld  a7, 17*8(a0)
    # x18-x27
    # ld  s2, 18*8(a0)
    # ld  s3, 19*8(a0)
    # ld  s4, 20*8(a0)
    # ld  s5, 21*8(a0)
    # ld  s6, 22*8(a0)
    # ld  s7, 23*8(a0)
    # ld  s8, 24*8(a0)
    # ld  s9, 25*8(a0)
    # ld s10, 26*8(a0)
    # ld s11, 27*8(a0)
    # x27-x31
    ld  t3, 28*8(a0)
    ld  t4, 29*8(a0)
    ld  t5, 30*8(a0)
    ld  t6, 31*8(a0)
    // load a0
    ld a0, 10*8(a0)
    sret

fast_syscall_entry:
    sd  ra,   1*8(a0)
    sd  gp,   3*8(a0)
    sd  tp,   4*8(a0)
    // store a0
    csrr t2, sscratch
    sd t2, 10*8(a0)
    csrr t0, sstatus
    sd t0, 32*8(a0)
    csrr t1, sepc
    sd t1, 33*8(a0)
    // kernel stack
    ld sp, 34*8(a0)
    // parameter: a0: frame ptr, a1-a7: call
    mv a6, a0
    call fast_syscall_handler
    // -> (a0: return value, a1: frame ptr)
    csrw sscratch, a1
    ld t0, 32*8(a1)
    csrw sstatus, t0
    ld t1, 33*8(a1)
    csrw sepc, t1       # user entry
    ld  ra,   1*8(a1)
    ld  sp,   2*8(a1)
    ld  gp,   3*8(a1)
    ld  tp,   4*8(a1)
    sret
