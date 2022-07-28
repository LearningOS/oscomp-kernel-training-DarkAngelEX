# Building
ARCH ?= riscv64
TARGET := riscv64imac-unknown-none-elf
MODE := debug
ADD_ARGS :=
ARGS :=
ARGS += $(ADD_ARGS)

KERNEL_ELF := target/$(TARGET)/$(MODE)/kernel
KERNEL_BIN := $(KERNEL_ELF).bin
DISASM_TMP := target/$(TARGET)/$(MODE)/asm
# FS_IMG := ../user/target/$(TARGET)/$(MODE)/fs.img
FS_IMG := ../user/target/$(TARGET)/release/fs.img
FS_IMG_BUILD := ../fat32-fuse
FS_IMG_SRC := ../fat32-fuse/fat32.img
SDCARD := /dev/sdb
APPS := ../user/src/bin/*

SMP := 2
MEMORY_SIZE := 128M

# BOARD
BOARD ?= qemu
# SBI ?= rustsbi
SBI ?= opensbi
BOOTLOADER := ../bootloader/$(SBI)-$(BOARD).bin
K210_BOOTLOADER_SIZE := 131072

# KERNEL ENTRY
ifeq ($(BOARD), qemu)
	KERNEL_ENTRY_PA := 0x80200000
else ifeq ($(BOARD), k210)
	KERNEL_ENTRY_PA := 0x80020000
endif

# Run K210
K210-SERIALPORT	= /dev/ttyUSB0
K210-BURNER = ../tools/kflash.py

# Binutils
OBJDUMP := rust-objdump --arch-name=riscv64
OBJCOPY := rust-objcopy --binary-architecture=riscv64

# Disassembly
DISASM ?= -x

build: env switch-check binary

switch-check:
ifeq ($(BOARD), qemu)
	(find last-qemu) || (rm -f last-k210 && touch last-qemu && make clean)
else ifeq ($(BOARD), k210)
	(find last-k210) || (rm -f last-qemu && touch last-k210 && make clean)
endif

env:
	(rustup target list | grep "riscv64imac-unknown-none-elf (installed)") || rustup target add $(TARGET)
	cargo install cargo-binutils --vers =0.3.3
	rustup component add rust-src
	rustup component add llvm-tools-preview

sdcard: fs-img
	@echo "Are you sure write to $(SDCARD) ? [y/N] " && read ans && [ $${ans:-N} = y ]
	@sudo dd if=/dev/zero of=$(SDCARD) bs=1048576 count=32
	@sudo dd if=$(FS_IMG) of=$(SDCARD)

binary: kernel
	@$(OBJCOPY) $(KERNEL_ELF) --strip-all -O binary $(KERNEL_BIN)
	@cp -f $(KERNEL_BIN) ./os.bin

fs-img-prev: $(APPS)
	@cd ../user && make build
	@rm -f $(FS_IMG)
#	@cd ../easy-fs-fuse && cargo run --release -- -s ../user/src/bin/ -t ../user/target/riscv64imac-unknown-none-elf/release/ -o ../user/target/riscv64imac-unknown-none-elf/release/
#	@cd ../easy-fs-fuse && cargo run --release -- -s ../user/busybox_lua_testsuites/ -t ../user/busybox_lua_testsuites/ -o ../user/target/riscv64imac-unknown-none-elf/release/

fs-img:
#	@cd ../user && make submit
	@cd $(FS_IMG_BUILD) && make init
	@cp $(FS_IMG_SRC) $(FS_IMG)

$(APPS):

kernel:
ifeq ($(ARCH), riscv64)
ifeq ($(BOARD), qemu)
	@echo Platform: qemu
	@cp src/hart/boot/linker64.ld src/linker.ld
ifeq ($(MODE), release)
	@cargo build --release $(ARGS)
else
	cargo build $(ARGS)
endif
	@rm src/linker.ld
endif
endif


take_bin: binary

clean:
	@cargo clean

disasm: kernel
	@$(OBJDUMP) $(DISASM) $(KERNEL_ELF) | less

disasm-vim: kernel
	@$(OBJDUMP) $(DISASM) $(KERNEL_ELF) > $(DISASM_TMP)
	@vim $(DISASM_TMP)
	@rm $(DISASM_TMP)

run: run-inner


run-inner: build
ifeq ($(BOARD),qemu)
	@qemu-system-riscv64 \
		-machine virt \
		-nographic \
		-smp $(SMP) \
		-bios $(BOOTLOADER) \
		-m $(MEMORY_SIZE) \
		-device loader,file=$(KERNEL_BIN),addr=$(KERNEL_ENTRY_PA) \
		-drive file=$(FS_IMG),if=none,format=raw,id=x0 \
        -device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0
else
	(which $(K210-BURNER)) || (cd .. && git clone https://github.com/sipeed/kflash.py.git && mv kflash.py tools)
	@cp $(BOOTLOADER) $(BOOTLOADER).copy
	@dd if=$(KERNEL_BIN) of=$(BOOTLOADER).copy bs=$(K210_BOOTLOADER_SIZE) seek=1
	@mv $(BOOTLOADER).copy $(KERNEL_BIN)
	@sudo chmod 777 $(K210-SERIALPORT)
	python3 $(K210-BURNER) -p $(K210-SERIALPORT) -b 1500000 $(KERNEL_BIN)
	python3 -m serial.tools.miniterm --eol LF --dtr 0 --rts 0 --filter direct $(K210-SERIALPORT) 115200
endif

debug: build
	@tmux new-session -d \
		"qemu-system-riscv64 -machine virt -nographic -bios $(BOOTLOADER) -m $(MEMORY_SIZE) \
		-device loader,file=$(KERNEL_BIN),addr=$(KERNEL_ENTRY_PA) \
		-drive file=$(FS_IMG),if=none,format=raw,id=x0 \
		-device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0 \
		-s -S" && \
		tmux split-window -h "riscv64-unknown-elf-gdb -ex 'file $(KERNEL_ELF)' -ex 'set arch riscv:rv64' -ex 'target remote localhost:1234'" && \
		tmux -2 attach-session -d

drun: build
	@qemu-system-riscv64 -machine virt -nographic -bios $(BOOTLOADER) -m $(MEMORY_SIZE) \
		-device loader,file=$(KERNEL_BIN),addr=$(KERNEL_ENTRY_PA) \
		-drive file=$(FS_IMG),if=none,format=raw,id=x0 \
		-device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0 \
		-s -S

#	-ex 'file ../disk/entry-dynamic.exe'
dgdb:
	@riscv64-unknown-elf-gdb \
		-ex 'file $(KERNEL_ELF)' \
		-ex 'set arch riscv:rv64' \
		-ex 'target remote localhost:1234' \

#qemu-riscv64

.PHONY: build env kernel clean disasm disasm-vim run-inner switch-check
