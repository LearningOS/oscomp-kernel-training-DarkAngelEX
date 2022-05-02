mkdir test_case
cp -r testsuits-for-oskernel/riscv-syscalls-testing/user/build/riscv64 test_case
cp -f user/target/riscv64gc-unknown-none-elf/release/initproc initproc/
cp -f user/target/riscv64gc-unknown-none-elf/release/user_shell initproc/
