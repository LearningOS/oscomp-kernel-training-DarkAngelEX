if [ -d test_case ]; then rm -rf test_case; fi; mkdir test_case;
if [ -d initproc  ]; then rm -rf initproc;  fi; mkdir initproc;
cp -r testsuits-for-oskernel/riscv-syscalls-testing/user/build/riscv64/. test_case/
cp -f user/target/riscv64gc-unknown-none-elf/release/initproc initproc/
cp -f user/target/riscv64gc-unknown-none-elf/release/user_shell initproc/
cp -f user/target/riscv64gc-unknown-none-elf/release/run_all_case initproc/
