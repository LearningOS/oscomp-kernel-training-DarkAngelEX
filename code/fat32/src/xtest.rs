use core::{future::Future, pin::Pin, task::Waker};

use alloc::{boxed::Box, sync::Arc};

use crate::{
    fat_list::FatList,
    layout::{bpb::RawBPB, fsinfo::RawFsInfo, name::NameSet},
    tools::{UtcTime, CID},
    BlockDevice, DirInode, Fat32Manager,
};

pub async fn test(
    device: impl BlockDevice,
    utc_time: impl Fn() -> UtcTime + Send + Sync + 'static,
    spawn_fn: impl FnMut(Pin<Box<dyn Future<Output = ()> + Send + 'static>>) + Clone + Send + 'static,
) {
    stack_trace!();
    println!("test start!");
    let device = Arc::new(device);
    info_test(device.clone()).await;
    system_test(device.clone(), Box::new(utc_time), spawn_fn).await;
    println!("test end!");
}

fn system_test(
    device: Arc<dyn BlockDevice>,
    utc_time: Box<dyn Fn() -> UtcTime + Send + Sync + 'static>,
    spawn_fn: impl FnMut(Pin<Box<dyn Future<Output = ()> + Send + 'static>>) + Clone + Send + 'static,
) -> impl Future<Output = ()> + Send + 'static {
    async fn show_dir(dir: &DirInode, manager: &Fat32Manager) {
        for (i, name) in dir.list(&manager).await.unwrap().into_iter().enumerate() {
            println!("{:>2} <{}>", i, name);
        }
    }
    async move {
        let mut manager = Fat32Manager::new(100, 100, 100, 100, 100);
        manager.init(device, utc_time).await;
        let root = manager.search_dir(&[]).await.unwrap();
        println!("/// show file ///");
        show_dir(&root, &manager).await;
        println!("/// show dir0 ///");
        let dir0 = root.search_dir(&manager, "dir0").await.unwrap();
        show_dir(&dir0, &manager).await;
        manager.spawn_sync_task((2, 2), spawn_fn).await;
        root.create_dir(&manager, "dir2", false, false).await.unwrap();
        println!("/// after insert dir2 ///");
        show_dir(&root, &manager).await;
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
    nameset.show(30);

    // let mut nameset = NameSet::new(&bpb);
    // nameset.load(&bpb, CID(3), &*device).await;
    // nameset.show(0);
}
