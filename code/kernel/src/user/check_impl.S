    .section .text
    .attribute arch, "rv64imac"
    .global __try_read_user_u8
    .global __try_write_user_u8
    .global __try_write_user_u32_atomic
    .global __try_access_user_error_trap
    .global __try_access_user_error_vector
    .align 6
// 在测试之前需要先设置错误处理
__try_read_user_u8:
    mv a1, a0
    mv a0, zero
    lb a1, 0(a1)
    ret
    .align 2
__try_write_user_u8:
    mv a2, a0
    mv a0, zero
    sb a1, 0(a2)
    ret
    .align 2
__try_write_user_u32_atomic:
    mv a1, a0
    mv a0, zero
    amoadd.w a1, a0, (a1)
    ret

// if pagefault occurs, (a0, a1) <- (1, scause).
    .align 6
__try_access_user_error_trap:
    addi sp, sp, -16
    sd   ra, 8(sp)
    csrr ra, scause
    bltz ra, access_interrupt
    csrw sepc, ra
    li   a0, 1
    csrr a1, scause
    ld   ra, 8(sp)
    addi sp, sp, 16
    sret
access_interrupt:
    ld   ra, 8(sp)
    addi sp, sp, 16
    j __kernel_default_interrupt_entry

// if page fault happen, (a0, a1) <- (1, scause).
try_access_user_error:
    csrw sepc, ra
    li   a0, 1
    csrr a1, scause
    sret

.align 8
__try_access_user_error_vector:
    j try_access_user_error
    .rept 16
    .align 2
    j __kernel_default_interrupt_entry
    .endr
