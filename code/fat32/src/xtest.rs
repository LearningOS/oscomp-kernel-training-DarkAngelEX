use crate::{
    fat_list::FatList,
    layout::{bpb::RawBPB, fsinfo::RawFsInfo, name::NameSet},
    tools::CID,
    BlockDevice,
};

pub async fn test(device: impl BlockDevice) {
    stack_trace!();
    println!("test start!");
    info_test(&device).await;
    println!("test end!");
}

async fn info_test(device: &dyn BlockDevice) {
    let mut bpb = RawBPB::zeroed();
    bpb.load(device).await;
    println!("{}\n", bpb);

    let mut fsinfo = RawFsInfo::zeroed();
    fsinfo.load(&bpb, device).await;
    println!("{}\n", fsinfo);

    let mut fat_list = FatList::empty();
    fat_list.load(&bpb, 0, device).await;
    fat_list.show(10);
    println!();

    let mut nameset = NameSet::new(&bpb);
    nameset.load(&bpb, CID(2), device).await;
    nameset.show(0);
}
