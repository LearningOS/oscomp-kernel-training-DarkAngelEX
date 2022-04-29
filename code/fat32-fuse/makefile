IMG_PATH = ./fat32.img
DIR_PATH = ./img_test

all: clean init

create: clean
	@rm -rf $(IMG_PATH)
	@rm -rf $(DIR_PATH)
	dd if=/dev/zero of=$(IMG_PATH) bs=40M count=1
	mkdir $(DIR_PATH)
	mkfs.vfat -s 8 -F 32 $(IMG_PATH)

init: create mount init_dir_0 umount

mount:
	sudo mount -t vfat -o user,umask=000,utf8=1 --source $(IMG_PATH) --target $(DIR_PATH)

umount:
	sudo umount $(DIR_PATH)


init_dir_0:
	@cd $(DIR_PATH) && \
	mkdir dir0 && \
	touch dir0/a && \
	mkdir dir1 && \
	touch dir1/b && \
	touch 'abcde' && \
	touch '居然是中文!' && \
	touch touch 12345678901234567890123456 && \
	touch end

init_dir_1:
	@cd $(DIR_PATH) && \
	mkdir dir0 && \
	touch dir0/a && \
	mkdir dir1 && \
	touch dir1/b && \
	touch '!' && \
	# " \
	touch '#' && \
	touch '$$' && \
	touch '%' && \
	touch '&' && \
	touch '(' && \
	touch ')' && \
	# * \
	touch _~1 && \
	touch '+' && \
	touch ',' && \
	touch '-' && \
	touch '1-2' && \
	# . \
	# / \
	# : \
	touch ';' && \
	# < \
	touch '=' && \
	# > \
	# ? \
	touch '@' && \
	touch '[' && \
	# / \
	touch ']' && \
	touch '^' && \
	touch '_' && \
	touch '`' && \
	touch '{' && \
	touch '}' && \
	touch '~' && \
	# \
	touch 'a~b~c00002' && \
	touch 'a~b~c00001' && \
	touch 'a' && \
	touch 'A' && \
	touch 12345678 && \
	touch 123456789 && \
	touch 1.2345 && \
	touch '1 2 3 4.4 5 6 7' && \
	touch '1.4+5#' && \
	touch N.M && \
	touch A..M && \
	touch A.B.M && \
	touch .U && \
	touch ..V && \
	touch ...W && \
	touch X. && \
	touch Y.. && \
	touch Z... && \
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
	touch 12345678901234567890123456.789 && \
	touch 12345678901234567890123456.0 && \
	touch looooooo0 && \
	touch looooooo1 && \
	touch looooooo2 && \
	touch looooooo3 && \
	touch looooooo4 && \
	touch looooooo5 && \
	touch looooooo6 && \
	touch looooooo7 && \
	touch looooooo8 && \
	touch "好啊22.455" && \
	touch BIG4K && truncate -s 4096 BIG4K && \
	touch BIG6K && truncate -s 6000 BIG6K && \
	touch BIG8K && truncate -s 8192 BIG8K && \
	touch BIGTEST && truncate -s 4096 BIGTEST && truncate -s 0 BIGTEST && \
	touch endfile

.PHONY: clean

test:
#	@cargo run --release -- -s $(IMG_PATH)
	@RUST_BACKTRACE=1 cargo run -- -s $(IMG_PATH)

clean:
	@rm -rf $(IMG_PATH)
	@rm -rf $(DIR_PATH)
