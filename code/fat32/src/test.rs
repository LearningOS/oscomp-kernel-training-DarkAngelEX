use alloc::boxed::Box;

use crate::{
    fat_list::FatList,
    layout::{bpb::RawBPB, fsinfo::RawFsInfo, name::NameSet},
    BlockDevice, tools::CID,
};

pub async fn test(device: impl BlockDevice) {
    stack_trace!();
    println!("test start!");
    let mut buf = unsafe { Box::new_uninit_slice(device.sector_bytes()).assume_init() };
    device.read_block(0, &mut buf).await.unwrap();
    let mut bpb = RawBPB::zeroed();
    bpb.raw_load(&buf);
    println!("{}\n", bpb);

    let mut fsinfo = RawFsInfo::zeroed();
    device.read_block(1, &mut buf).await.unwrap();
    fsinfo.raw_load(&buf);
    println!("{}\n", fsinfo);

    let mut fat_list = FatList::empty();
    fat_list.load(&bpb, 0, &device).await;
    fat_list.show(10);
    println!();

    let mut nameset = NameSet::new(&bpb);
    nameset.load(&bpb, CID(0), &device).await;
    nameset.show(0);

    println!("test end!");
}
