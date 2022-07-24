use core::future::Future;

use alloc::{boxed::Box, sync::Arc};
use ftl_util::device::BlockDevice;
use vfs::{VfsClock, VfsSpawner};

use crate::{
    fat_list::FatList,
    layout::{bpb::RawBPB, fsinfo::RawFsInfo, name::NameSet},
    tools::CID,
    DirInode, Fat32Manager,
};

pub async fn test(
    device: impl BlockDevice,
    clock: Box<dyn VfsClock>,
    spawner: Box<dyn VfsSpawner>,
) {
    stack_trace!();
    println!("test start!");
    let device = Arc::new(device);
    info_test(device.clone()).await;
    system_test(device.clone(), clock, spawner).await;
    println!("test end!");
}

fn system_test(
    device: Arc<dyn BlockDevice>,
    clock: Box<dyn VfsClock>,
    spawner: Box<dyn VfsSpawner>,
) -> impl Future<Output = ()> + Send + 'static {
    async fn show_dir(dir: &DirInode, manager: &Fat32Manager) {
        for (i, (dt, name)) in dir.list(&manager).await.unwrap().into_iter().enumerate() {
            println!("{:>2} <{}> {:?}", i, name, dt);
        }
    }
    async move {
        let mut manager = Fat32Manager::new(0, 100, 100, 100, 100, 100);
        manager.init(device, clock).await;
        let root = manager.search_dir(&[]).await.unwrap();
        println!("/// show file ///");
        show_dir(&root, &manager).await;
        println!("/// show dir0 ///");
        let dir0 = root.search_dir(&manager, "dir0").await.unwrap();
        show_dir(&dir0, &manager).await;
        manager.spawn_sync_task((2, 2), spawner).await;
        println!("/// create dir2 0 ///");
        root.create_dir(&manager, "dir2", false, false)
            .await
            .unwrap();
        println!("/// delete dir2 0 ///");
        root.delete_dir(&manager, "dir2").await.unwrap();
        println!("/// create dir2 1 ///");
        root.create_dir(&manager, "dir2", false, false)
            .await
            .unwrap();
        show_dir(&root, &manager).await;
        println!("/// dir2 create bbb ///");
        let dir2 = root.search_dir(&manager, "dir2").await.unwrap();
        dir2.create_file(&manager, "bbb", false, false)
            .await
            .unwrap();
        println!("/// write bbb ///");
        let bbb = dir2.search_file(&manager, "bbb").await.unwrap();
        bbb.write_append(&manager, b"123456").await.unwrap();
        println!("/// read abcde ///");
        let abcde = root.search_file(&manager, "abcde").await.unwrap();
        let buffer = &mut [0; 20];
        let n = abcde.read_at(&manager, 0, buffer).await.unwrap();
        let str = unsafe { core::str::from_utf8_unchecked(&buffer[..n]) };
        println!("read:<{}> n:{}", str, n);
        println!("/// delete abcde ///");
        drop(abcde);
        root.delete_file(&manager, "abcde").await.unwrap();
        println!("/// delete bbb ///");
        drop(bbb);
        dir2.delete_file(&manager, "bbb").await.unwrap();
        println!("/// delete dir2 ///");
        drop(dir2);
        root.delete_dir(&manager, "dir2").await.unwrap();
        println!("/// test end ///");
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
}
