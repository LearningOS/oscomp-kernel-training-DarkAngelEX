    .section .text.trampoline
    .global __trap_entry
    .global __exec_return
    .global __fork_return
    .align 2
// ENTRY_COMMON: save ra, sp, gp, tp, sstatus, sepc, load sp(kernel)
//
// RETURN_COMMON: load sscratch(frame), sstatus, sepc, ra, sp, gp, tp
//
// __trap_entry => ENTRY_COMMON -> judge_syscall -> trap_handler_entry / syscall_entry
//
// trap_handler_entry => save t0-t6, a0-a7 -> call trap_handler -> RETURN_COMMON -> load t0-t6, a0-a7 -> sret
//
// syscall_entry => mv a6, user a0 -> call syscall_handler -> judge syscall_return / trap_after_save_sx_entry
//
// syscall_return => RETURN_COMMON -> set a0(return value) -> sret
//
// trap_after_save_sx_entry => save s0-s11 -> call trap_after_save_sx -> syscall_return
//
// __exec_return => RETURN_COMMON -> load a0, a1 -> sret
//
// __fork_return => RETURN_COMMON -> load s0-s11, a0 ->sret

.altmacro

.macro print ch
    mv t0, a7
    mv t1, a0
    li a7, 1
    li a0, \ch
    ecall
    mv a7, t0
    mv a0, t1
.endm

.macro ENTRY_COMMON f
    csrrw \f, sscratch, \f
    sd   ra,   1*8(\f)
    sd   sp,   2*8(\f)
    sd   gp,   3*8(\f)
    sd   tp,   4*8(\f) # save user tp
    csrr ra,  sstatus
    csrr sp,  sepc
    sd   ra,  32*8(\f)
    sd   sp,  33*8(\f)
    ld   sp,  34*8(\f)
    ld   tp,  35*8(\f) # load kernel tp(hart id)
.endm

.macro RETURN_COMMON f
    # ra sp will be load then.
    ld   ra,  32*8(\f)
    ld   sp,  33*8(\f)
    csrw sscratch, \f
    csrw sstatus,  ra
    csrw sepc,     sp
    # swap tp, 4*8(\f)
    sd   tp,  35*8(\f) # save kernel tp(hart id)
    ld   ra,   1*8(\f)
    ld   sp,   2*8(\f)
    ld   gp,   3*8(\f)
    ld   tp,   4*8(\f) # load user tp
.endm

.macro judge_syscall t goto
    csrr \t,  scause
    addi \t,  \t, -8
    beqz \t,  \goto
.endm

.macro SAVE_TX f
    sd  t0,   5*8(\f)
    sd  t1,   6*8(\f)
    sd  t2,   7*8(\f)
    sd  t3,  28*8(\f)
    sd  t4,  29*8(\f)
    sd  t5,  30*8(\f)
    sd  t6,  31*8(\f)
.endm

.macro LOAD_TX f
    ld  t0,   5*8(\f)
    ld  t1,   6*8(\f)
    ld  t2,   7*8(\f)
    ld  t3,  28*8(\f)
    ld  t4,  29*8(\f)
    ld  t5,  30*8(\f)
    ld  t6,  31*8(\f)
.endm

.macro SAVE_SX f
    sd  s0,  8*8(\f)
    sd  s1,  9*8(\f)
    sd  s2, 18*8(\f)
    sd  s3, 19*8(\f)
    sd  s4, 20*8(\f)
    sd  s5, 21*8(\f)
    sd  s6, 22*8(\f)
    sd  s7, 23*8(\f)
    sd  s8, 24*8(\f)
    sd  s9, 25*8(\f)
    sd s10, 26*8(\f)
    sd s11, 27*8(\f)
.endm

.macro LOAD_SX f
    ld  s0,  8*8(\f)
    ld  s1,  9*8(\f)
    ld  s2, 18*8(\f)
    ld  s3, 19*8(\f)
    ld  s4, 20*8(\f)
    ld  s5, 21*8(\f)
    ld  s6, 22*8(\f)
    ld  s7, 23*8(\f)
    ld  s8, 24*8(\f)
    ld  s9, 25*8(\f)
    ld s10, 26*8(\f)
    ld s11, 27*8(\f)
.endm

__trap_entry:
    // 0(f)-31*8(f) x0-x31 a0 is x10
    // 32*8(f) sstatus
    // 33*8(f) sepc
    // 34*8(f) kernel_sp
    // 35*8(f) kernel_tp
    // 36*8(f) need_add_task
    ENTRY_COMMON a0
    judge_syscall ra syscall_entry
trap_handler_entry:
    SAVE_TX a0
    # skip a0
    sd  a1, 11*8(a0)
    sd  a2, 12*8(a0)
    sd  a3, 13*8(a0)
    sd  a4, 14*8(a0)
    sd  a5, 15*8(a0)
    sd  a6, 16*8(a0)
    sd  a7, 17*8(a0)
    csrr ra, sscratch
    sd  ra, 10*8(a0) // sd a0 there
    call trap_handler // -> a0: frame ptr
    RETURN_COMMON a0
    LOAD_TX a0
    // skip a0
    ld  a1, 11*8(a0)
    ld  a2, 12*8(a0)
    ld  a3, 13*8(a0)
    ld  a4, 14*8(a0)
    ld  a5, 15*8(a0)
    ld  a6, 16*8(a0)
    ld  a7, 17*8(a0)
    ld  a0, 10*8(a0)  // load a0 there
    sret

syscall_entry:
    csrr a6, sscratch // a6 <- (user a0)
    // parameter: a0: frame ptr, a1-a7: call, a6 is a0(user)
    call syscall_handler // -> (a0: return value, a1: frame ptr)
    ld t1, 36*8(a1)
    bnez t1, trap_after_save_sx_entry
    RETURN_COMMON a1
    sret

trap_after_save_sx_entry:
    ld t0, 37*8(a1)
    SAVE_SX t0
    // parameter: a0: from syscall a1: frame ptr
    call trap_after_save_sx // -> (a0: return value, a1: frame ptr)
    RETURN_COMMON a1
    sret

__exec_return: // -> (a0: frame)
    RETURN_COMMON a0
    ld  a1, 11*8(a0) // argc
    ld  a0, 10*8(a0) // argv
    sret

__fork_return: // -> (a0: frame)
    RETURN_COMMON a0
    LOAD_SX a0
    ld  a0, 10*8(a0) // return value
    sret

# ========================== DEBUG ==========================


print_a0_num:
    mv s1, ra
    print '0'
    print 'x'
    mv s0, a0
    srli a0, s0, 15*4
    call print_a0_single
    srli a0, s0, 14*4
    call print_a0_single
    srli a0, s0, 13*4
    call print_a0_single
    srli a0, s0, 12*4
    call print_a0_single
    srli a0, s0, 11*4
    call print_a0_single
    srli a0, s0, 10*4
    call print_a0_single
    srli a0, s0, 9*4
    call print_a0_single
    srli a0, s0, 8*4
    call print_a0_single
    srli a0, s0, 7*4
    call print_a0_single
    srli a0, s0, 6*4
    call print_a0_single
    srli a0, s0, 5*4
    call print_a0_single
    srli a0, s0, 4*4
    call print_a0_single
    srli a0, s0, 3*4
    call print_a0_single
    srli a0, s0, 2*4
    call print_a0_single
    srli a0, s0, 1*4
    call print_a0_single
    srli a0, s0, 0*4
    call print_a0_single
    mv a0, s0
    mv ra, s1
    ret

print_a0_single:
    mv s2, a0
    andi a0, a0, 0xf
    li t0, 0
    beq a0, t0, print_0
    addi t0, t0, 1
    beq a0, t0, print_1
    addi t0, t0, 1
    beq a0, t0, print_2
    addi t0, t0, 1
    beq a0, t0, print_3
    addi t0, t0, 1
    beq a0, t0, print_4
    addi t0, t0, 1
    beq a0, t0, print_5
    addi t0, t0, 1
    beq a0, t0, print_6
    addi t0, t0, 1
    beq a0, t0, print_7
    addi t0, t0, 1
    beq a0, t0, print_8
    addi t0, t0, 1
    beq a0, t0, print_9
    addi t0, t0, 1
    beq a0, t0, print_A
    addi t0, t0, 1
    beq a0, t0, print_B
    addi t0, t0, 1
    beq a0, t0, print_C
    addi t0, t0, 1
    beq a0, t0, print_D
    addi t0, t0, 1
    beq a0, t0, print_E
    addi t0, t0, 1
    beq a0, t0, print_F
    unimp
print_0:
    print '0'
    j print_end
print_1:
    print '1'
    j print_end
print_2:
    print '2'
    j print_end
print_3:
    print '3'
    j print_end
print_4:
    print '4'
    j print_end
print_5:
    print '5'
    j print_end
print_6:
    print '6'
    j print_end
print_7:
    print '7'
    j print_end
print_8:
    print '8'
    j print_end
print_9:
    print '9'
    j print_end
print_A:
    print 'A'
    j print_end
print_B:
    print 'B'
    j print_end
print_C:
    print 'C'
    j print_end
print_D:
    print 'D'
    j print_end
print_E:
    print 'E'
    j print_end
print_F:
    print 'F'
    j print_end
print_end:
    mv a0, s2
    ret
