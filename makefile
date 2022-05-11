KERNEL_DIR = ./code/kernel

native:
	cd $(KERNEL_DIR) && make take_bin ADD_ARGS="--offline --features board_hifive"
	cp $(KERNEL_DIR)/os.bin ./os.bin

all:
	cd $(KERNEL_DIR) && make take_bin ADD_ARGS='--offline --features "board_hifive submit"'
	cp $(KERNEL_DIR)/os.bin ./os.bin
