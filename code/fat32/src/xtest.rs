use core::future::Future;

use alloc::{boxed::Box, sync::Arc};

use crate::{
    fat_list::FatList,
    layout::{bpb::RawBPB, fsinfo::RawFsInfo, name::NameSet},
    tools::{UtcTime, CID},
    BlockDevice, Fat32Manager,
};

pub async fn test(
    device: impl BlockDevice,
    utc_time: impl Fn() -> UtcTime + Send + Sync + 'static,
) {
    stack_trace!();
    println!("test start!");
    let device = Arc::new(device);
    info_test(device.clone()).await;
    system_test(device.clone(), Box::new(utc_time)).await;
    println!("test end!");
}

// fn system_test(
//     device: Arc<dyn BlockDevice>,
//     utc_time: Box<dyn Fn() -> UtcTime + Send + 'static>,
// ) -> impl Future<Output = ()> + Send + 'static {
//     async move {system_test_impl(device, utc_time).await}
// }

fn system_test(
    device: Arc<dyn BlockDevice>,
    utc_time: Box<dyn Fn() -> UtcTime + Send + Sync + 'static>,
) -> impl Future<Output = ()> + Send + 'static {
    async move {
        let mut manager = Fat32Manager::new(100, 100, 100, 100, 100);
        manager.init(device, utc_time).await;
        let root = manager.search_dir(&[]).await.unwrap();
        for (i, name) in root.list(&manager).await.unwrap().into_iter().enumerate() {
            println!("{:>2} {}", i, name);
        }
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
