KERNEL_DIR = ./code/kernel
USER_DIR = ./code/user

native:
	cd $(KERNEL_DIR) && make take_bin ADD_ARGS="--offline --features board_hifive"
	cp $(KERNEL_DIR)/os.bin ./os.bin

all_old:
	cd $(USER_DIR) && make submit ADD_ARGS='--offline'
	cd $(KERNEL_DIR) && make take_bin ADD_ARGS='--offline --features "board_hifive submit"' MODE=release
	cp $(KERNEL_DIR)/os.bin ./os.bin

# 这一行会产生init程序, 只需要运行一次(非常慢)
user:
#	cd $(USER_DIR) && make submit ADD_ARGS='--offline'
	cd $(USER_DIR) && make submit ADD_ARGS=''

# 第一次运行需要make user
kernel:
	cd $(KERNEL_DIR) && make take_bin ADD_ARGS='--features "submit"' MODE=release
#	cd $(KERNEL_DIR) && make take_bin ADD_ARGS='--features "stack_trace submit"' MODE=debug
#	cd $(KERNEL_DIR) && make take_bin ADD_ARGS='--features "stack_trace"' MODE=debug
#	cd $(KERNEL_DIR) && make take_bin ADD_ARGS='--features "stack_trace"' MODE=release
	cp $(KERNEL_DIR)/os.bin ./kernel-qemu

# 评测机会运行这一行
all: user kernel

fs:
	cp fat32src.img fat32.img -f

# 使用根目录提供的fat32.img, 这是评测机提供的
run: kernel
	qemu-system-riscv64 \
    -machine virt \
    -bios default \
    -device loader,file=kernel-qemu,addr=0x80200000 \
    -drive file=fat32.img,if=none,format=raw,id=x0 \
    -device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0 \
    -kernel kernel-qemu \
    -nographic \
    -smp 2 -m 2G

drun: kernel
	@qemu-system-riscv64 -machine virt -nographic -bios default -m 2G \
		-device loader,file=fat32.img,addr=0x80200000 \
		-drive file=fat32.img,if=none,format=raw,id=x0 \
		-device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0 \
		-s -S

dgdb:
	@riscv64-unknown-elf-gdb \
		-ex 'file ../init/lmbench_all' \
		-ex 'set arch riscv:rv64' \
		-ex 'target remote localhost:1234' \
