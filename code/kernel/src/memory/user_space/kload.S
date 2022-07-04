.section .data
.global __kload_begin
.global __user_signal_entry_begin
.global __user_signal_entry_end

.align 6
__kload_begin:
__user_signal_entry_begin:
    li a7, 139
    ecall
    unimp
__user_signal_entry_end:
__kload_end: