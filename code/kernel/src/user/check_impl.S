    .section .text
    .global __try_read_user_u8
    .global __try_write_user_u8
    .global __try_access_user_error_vector
    .align 2
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

// if page fault happen, a0 will become 1, a1 will become scause.
    .align 2
__try_access_user_error_trap:
    addi sp, sp, -16
    sd   ra, 8(sp)
    # call try_access_user_error_debug
    csrr a0, sepc
    addi a0, a0, 4
    csrw sepc, a0
    li   a0, 1
    csrr a1, scause
    ld   ra, 8(sp)
    addi sp, sp, 16
    sret
