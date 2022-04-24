use core::future::Future;

use alloc::{boxed::Box, sync::Arc};

use crate::{
    access::AccessPath,
    fat_list::FatList,
    layout::{bpb::RawBPB, fsinfo::RawFsInfo, name::NameSet},
    tools::{UtcTime, CID},
    BlockDevice, Fat32Manager,
};

pub async fn test(device: impl BlockDevice, utc_time: impl Fn() -> UtcTime + Send + 'static) {
    stack_trace!();
    println!("test start!");
    let device = Arc::new(device);
    info_test(device.clone()).await;
    system_test(device.clone(), Box::new(utc_time)).await;
    println!("test end!");
}

fn system_test(
    device: Arc<dyn BlockDevice>,
    utc_time: Box<dyn Fn() -> UtcTime + Send + 'static>,
) -> impl Future<Output = ()> + Send + 'static {
    async move {
        let mut fat32 = Fat32Manager::new(100, 100, 100, 100, 100);
        fat32.init(device, utc_time).await;
        let path = AccessPath::new();
        todo!()
        // let root = fat32.access(&path).await.unwrap();
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
    fat_list.show(20).await;
    println!();

    let mut nameset = NameSet::new(&bpb);
    nameset.load(&bpb, CID(2), &*device).await;
    nameset.show(0);
}
