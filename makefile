KERNEL_DIR = ./code/kernel

all:
	cd $(KERNEL_DIR) && make build
	cp $(KERNEL_DIR)/os.bin ./os.bin