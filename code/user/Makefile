TARGET := riscv64imac-unknown-none-elf
ADD_ARGS := ""
MODE := release
APP_DIR := src/bin
TARGET_DIR := target/$(TARGET)/$(MODE)
APPS := $(wildcard $(APP_DIR)/*.rs)
ELFS := $(patsubst $(APP_DIR)/%.rs, $(TARGET_DIR)/%, $(APPS))
BINS := $(patsubst $(APP_DIR)/%.rs, $(TARGET_DIR)/%.bin, $(APPS))

OBJDUMP := rust-objdump --arch-name=riscv64
OBJCOPY := rust-objcopy --binary-architecture=riscv64

elf: $(APPS)
ifeq ($(MODE), release)
	@cargo build --release $(ADD_ARGS)
else
	@cargo build $(ADD_ARGS)
endif

binary: elf
	$(foreach elf, $(ELFS), $(OBJCOPY) $(elf) --strip-all -O binary $(patsubst $(TARGET_DIR)/%, $(TARGET_DIR)/%.bin, $(elf));)

build: binary

submit: build
	@-(find ../kernel/run_all_case) && (rm -rf ../kernel/run_all_case)
	cp -f $(TARGET_DIR)/run_all_case ../kernel/run_all_case

clean:
	@cargo clean

.PHONY: elf binary build clean
