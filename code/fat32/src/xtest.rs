use alloc::sync::Arc;

use crate::{
    access::AccessPath,
    fat_list::FatList,
    layout::{bpb::RawBPB, fsinfo::RawFsInfo, name::NameSet},
    tools::CID,
    BlockDevice, Fat32Manager,
};

pub async fn test(device: impl BlockDevice) {
    stack_trace!();
    println!("test start!");
    let device = Arc::new(device);
    info_test(&*device).await;
    system_test(device.clone()).await;
    println!("test end!");
}

async fn system_test(device: Arc<dyn BlockDevice>) {
    let mut fat32 = Fat32Manager::new(100);
    fat32.init(device).await;
    let path = AccessPath::new();
    let root = fat32.access(&path).await.unwrap();
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
