IMG_PATH = ./fat32.img
DIR_PATH = ./img_test

all: clean init

create: clean
	@rm -rf $(IMG_PATH)
	@rm -rf $(DIR_PATH)
	dd if=/dev/zero of=$(IMG_PATH) bs=40M count=1
	mkdir $(DIR_PATH)
	mkfs.vfat -s 8 -F 32 $(IMG_PATH)

init: create
	sudo mount -t vfat -o user,umask=000 --source $(IMG_PATH) --target $(DIR_PATH)
	@cd $(DIR_PATH) && \
	touch aaa. && \
	touch .a && \
	touch .aa && \
	touch bbb && \
	touch .bbb && \
	touch .ccccc && \
	touch d.dddd && \
	touch EEE && \
	touch noext && \
	touch short.txt && \
	touch looooooong12 && \
	touch loooooooong13 && \
	touch looooooooong14 && \
	touch 12345678901234567890123456 && \
	cd ..
	sudo umount $(DIR_PATH)

.PHONY: clean

test:
	@cargo run --release -- -s $(IMG_PATH)

clean:
	@rm -rf $(IMG_PATH)
	@rm -rf $(DIR_PATH)
