rust-objdump --arch-name=riscv64 --mattr=+m,+a,+d -all busybox_lua_testsuites/busybox > busybox.S
# riscv64-unknown-elf-objdump -d busybox_lua_testsuites/busybox > busybox.S
