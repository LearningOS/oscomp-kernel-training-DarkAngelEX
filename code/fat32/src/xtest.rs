use core::future::Future;

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
    info_test(device.clone()).await;
    system_test(device.clone()).await;
    println!("test end!");
}

fn system_test(device: Arc<dyn BlockDevice>) -> impl Future<Output = ()> + Send + 'static {
    async move {
        let mut fat32 = Fat32Manager::new(100, 100);
        fat32.init(device).await;
        let path = AccessPath::new();
        let root = fat32.access(&path).await.unwrap();
    }
}

async fn info_test(device: Arc<dyn BlockDevice>) {
    let mut bpb = RawBPB::zeroed();
    bpb.load(&*device).await;
    println!("{}\n", bpb);

    let mut fsinfo = RawFsInfo::zeroed();
    fsinfo.load(bpb.info_cluster_id as usize, &*device).await;
    println!("{}\n", fsinfo);

    let mut fat_list = FatList::empty(100, 100);
    fat_list.init(&bpb, 0, device.clone()).await;
    fat_list.show(10).await;
    println!();

    let mut nameset = NameSet::new(&bpb);
    nameset.load(&bpb, CID(2), &*device).await;
    nameset.show(0);
}
