KERNEL_DIR = ./code/kernel

all:
	cd $(KERNEL_DIR) && make take_bin
	cp $(KERNEL_DIR)/os.bin ./os.bin
