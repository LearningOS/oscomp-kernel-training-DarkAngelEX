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
__trap_entry:
    // 0(a0) - 31*8(a0) x0-x31 a0 is x10
    // 32*8(a0) sstatus
    // 33*8(a0) sepc
    // 34*8(a0) kernel_satp
    // 35*8(a0) kernel_sp
    // 36*8(a0) trap_handler
    csrrw a0, sscratch, a0
    sd x1,   1*8(a0)
    sd x2,   2*8(a0)
    sd x3,   3*8(a0)
    // skip tp(x4)
    sd x5,   5*8(a0)
    sd x6,   6*8(a0)
    sd x7,   7*8(a0)
    sd x8,   8*8(a0)
    sd x9,   9*8(a0)
    // skip x10
    sd x11, 11*8(a0)
    sd x12, 12*8(a0)
    sd x13, 13*8(a0)
    sd x14, 14*8(a0)
    sd x15, 15*8(a0)
    sd x16, 16*8(a0)
    sd x17, 17*8(a0)
    sd x18, 18*8(a0)
    sd x19, 19*8(a0)
    sd x20, 20*8(a0)
    sd x21, 21*8(a0)
    sd x22, 22*8(a0)
    sd x23, 23*8(a0)
    sd x24, 24*8(a0)
    sd x25, 25*8(a0)
    sd x26, 26*8(a0)
    sd x27, 27*8(a0)
    sd x28, 28*8(a0)
    sd x29, 29*8(a0)
    sd x30, 30*8(a0)
    sd x31, 31*8(a0)
    // store a0
    csrr t2, sscratch
    sd t2, 11*8(a0)

    csrr t0, sstatus
    csrr t1, sepc
    sd t0, 32*8(a0)
    sd t1, 33*8(a0)
    // set kernel_sp
    ld sp, 34*8(a0)
    // load trap_hander
    # ld t3, 36*8(sp)
    // call trap_handler !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
    # call t3

    call trap_handler
    # print 'b'
__trap_return:
    # print 'c'

    // return from same place.
    csrw sscratch, a0

    ld t0, 32*8(a0)
    ld t1, 33*8(a0)
    csrw sstatus, t0
    csrw sepc, t1

    ld x1,   1*8(a0)
    ld x2,   2*8(a0)
    ld x3,   3*8(a0)
    # skip tp(x4)
    ld x5,   5*8(a0)
    ld x6,   6*8(a0)
    ld x7,   7*8(a0)
    ld x8,   8*8(a0)
    ld x9,   9*8(a0)
    // skip x10
    ld x11, 11*8(a0)
    ld x12, 12*8(a0)
    ld x13, 13*8(a0)
    ld x14, 14*8(a0)
    ld x15, 15*8(a0)
    ld x16, 16*8(a0)
    ld x17, 17*8(a0)
    ld x18, 18*8(a0)
    ld x19, 19*8(a0)
    ld x20, 20*8(a0)
    ld x21, 21*8(a0)
    ld x22, 22*8(a0)
    ld x23, 23*8(a0)
    ld x24, 24*8(a0)
    ld x25, 25*8(a0)
    ld x26, 26*8(a0)
    ld x27, 27*8(a0)
    ld x28, 28*8(a0)
    ld x29, 29*8(a0)
    ld x30, 30*8(a0)
    ld x31, 31*8(a0)
    // load a0
    ld a0, 10*8(a0)
    sret
