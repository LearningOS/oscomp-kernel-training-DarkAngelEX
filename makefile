KERNEL_DIR = ./code/kernel

all:
	cd $(KERNEL_DIR) && make take_bin ADD_ARGS="--offline --features board_hifive"
	cp $(KERNEL_DIR)/os.bin ./os.bin
