KERNEL_DIR = ./code/kernel
USER_DIR = ./code/user

native:
	cd $(KERNEL_DIR) && make take_bin ADD_ARGS="--offline --features board_hifive"
	cp $(KERNEL_DIR)/os.bin ./os.bin

all:
	cd $(USER_DIR) && make submit ADD_ARGS='--offline'
	cd $(KERNEL_DIR) && make take_bin ADD_ARGS='--offline --features "board_hifive submit"'
	cp $(KERNEL_DIR)/os.bin ./os.bin
