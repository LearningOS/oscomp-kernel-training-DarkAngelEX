use alloc::{boxed::Box, sync::Arc, vec::Vec};
use ftl_util::device::BlockDevice;
use vfs::{VfsClock, VfsSpawner};

use crate::{
    fat_list::FatList,
    layout::{bpb::RawBPB, fsinfo::RawFsInfo, name::NameSet},
    tools::CID,
    DirInode, Fat32Manager,
};
#[cfg(test)]
mod driver;

#[cfg(test)]
fn init_console() {
    use std::io::Write;
    ftl_util::console::init(|a| std::io::stdout().write_fmt(a).unwrap());
}

#[test]
fn test_main() {
    init_console();
    // let path = "../fat32-fuse/fat32.img";
    let path = "../../fat32.img";

    let driver = driver::get_driver(path);
    let (executor, spawner) = ftl_util::async_tools::tiny_env::new_executor_and_spawner();
    spawner.spawn(imgtest(
        driver,
        Box::new(vfs::ZeroClock),
        Box::new(spawner.clone()),
    ));
    executor.run();
}

pub async fn test(
    device: Arc<dyn BlockDevice>,
    clock: Box<dyn VfsClock>,
    spawner: Box<dyn VfsSpawner>,
) {
    stack_trace!();
    println!("test start!");
    info_test(device.clone()).await;
    delete_test(device.clone(), clock, spawner).await;
    println!("test end!");
}

async fn show_dir(dir: &DirInode, manager: &Fat32Manager) {
    for (i, (dt, name)) in dir.list(manager).await.unwrap().into_iter().enumerate() {
        println!("{:>2} <{}> {:?}", i, name, dt);
    }
}
async fn base_test(
    device: Arc<dyn BlockDevice>,
    clock: Box<dyn VfsClock>,
    spawner: Box<dyn VfsSpawner>,
) {
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
    root.delete_file(&manager, "abcde", true).await.unwrap();
    println!("/// delete bbb ///");
    drop(bbb);
    dir2.delete_file(&manager, "bbb", true).await.unwrap();
    println!("/// delete dir2 ///");
    drop(dir2);
    root.delete_dir(&manager, "dir2").await.unwrap();
    println!("/// test end ///");
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

    println!("---------   show info end   ---------");
}

async fn delete_test(
    device: Arc<dyn BlockDevice>,
    clock: Box<dyn VfsClock>,
    spawner: Box<dyn VfsSpawner>,
) {
    let mut manager = Fat32Manager::new(0, 100, 100, 100, 100, 100);
    println!("--------- delete test begin ---------");
    manager.init(device, clock).await;
    manager.spawn_sync_task((2, 2), spawner).await;
    let manager = Arc::new(manager);

    macro_rules! create_file {
        ($dir: ident, $name: expr) => {
            $dir.create_file(&manager, $name, false, false).await
        };
    }
    macro_rules! search_file {
        ($dir: ident, $name: expr) => {
            $dir.search_file(&manager, $name).await
        };
    }

    let root = manager.root_dir();
    create_file!(root, "dead").unwrap();
    create_file!(root, "dead").unwrap_err(); // 禁止创建重复的文件
    let dead = search_file!(root, "dead").unwrap();

    let write = b"123";
    dead.write_at(&manager, 0, write).await.unwrap();
    dead.detach(&manager).await.unwrap();
    root.delete_file(&manager, "dead", false).await.unwrap(); // 删除文件
    let mut buffer = Vec::<u8>::new();
    buffer.resize(1000, 0);
    let n = dead.read_at(&manager, 0, &mut buffer[..]).await.unwrap();
    assert_eq!(write, &buffer[..n]); // 文件依然可以读出来
    create_file!(root, "dead").unwrap(); // 允许在原目录创建同一个文件
    drop(dead); // 这里会回收资源
    let dead = search_file!(root, "dead").unwrap();
    let n = dead.read_at(&manager, 0, &mut buffer[..]).await.unwrap();
    assert_eq!(0, n); // 新的文件什么也读不出来
}

pub async fn imgtest(
    device: Arc<dyn BlockDevice>,
    clock: Box<dyn VfsClock>,
    spawner: Box<dyn VfsSpawner>,
) {
    let mut manager = Fat32Manager::new(0, 100, 100, 100, 100, 100);
    manager.init(device, clock).await;
    let root = manager.search_dir(&[]).await.unwrap();
    println!("123434");
    // show_dir(&root, &manager).await;
    manager.spawn_sync_task((2, 2), spawner).await;
    let test_dir = root.search_dir(&manager, "test_dir").await.unwrap();
    show_dir(&test_dir, &manager).await;
    // root.create_file(&manager, ".ash_history", false, false)
    //     .await
    //     .unwrap();
}
