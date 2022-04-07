rm -rf ./fat32.img
rm -rf ./img_test
dd if=/dev/zero of=fat32.img bs=40M count=1
mkdir ./img_test
mkfs.vfat -F 32 fat32.img
chmod 777 fat32.img
sudo chmod 777 ./img_test
