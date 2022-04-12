rm -rf ./fat32.img
rm -rf ./img_test
dd if=/dev/zero of=fat32.img bs=40M count=1
mkdir ./img_test
mkfs.vfat -s 8 -F 32 fat32.img

