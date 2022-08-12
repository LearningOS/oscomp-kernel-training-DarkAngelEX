use alloc::{boxed::Box, string::ToString, sync::Arc};
use ftl_util::{
    async_tools::tiny_env,
    error::{SysError, SysR},
};

use crate::{
    manager::{ArcDevAlloc, ZeroClock},
    File, VfsFile, VfsManager,
};

#[cfg(test)]
fn init_console() {
    use std::io::Write;
    ftl_util::console::init(|a| std::io::stdout().write_fmt(a).unwrap());
}

#[test]
pub fn test_run() {
    init_console();
    let (executor, spawner) = tiny_env::new_executor_and_spawner();
    spawner.spawn(test_special());
    executor.run_debug();
}

fn xp(path: &str) -> (SysR<Arc<VfsFile>>, &str) {
    (Err(SysError::ENOENT), path)
}

/// 测试文件系统的目录层级创建是否可用
async fn test_create() {
    let rw = (true, true);
    let mut manager = VfsManager::new(10);
    manager.init_clock(Box::new(ZeroClock));
    manager.init_devalloc(Box::new(ArcDevAlloc::new()));
    manager.mount(xp(""), xp("/"), "tmpfs", 0).await.unwrap();
    let d0 = manager.create(xp("/0"), false, rw).await.unwrap();
    let d1 = manager.open(xp("/0")).await.unwrap();
    let src = b"123".as_slice();
    d0.write_at(0, src).await.unwrap();
    let dst = &mut [0; 100];
    let n = d1.read_at(0, dst).await.unwrap();
    assert_eq!(src.len(), n);
    assert_eq!(src, &dst[..n]);
    let _d2 = manager.create(xp("/1"), true, rw).await.unwrap();
    let _d3 = manager.create(xp("/1/2"), true, rw).await.unwrap();
    let _d4 = manager.create(xp("/1/2"), true, rw).await.unwrap_err();
    // 挂载点会覆盖目录
    manager.mount(xp(""), xp("/1"), "tmpfs", 0).await.unwrap();
    let _d4 = manager.create(xp("/1/2"), true, rw).await.unwrap();
}

/// 测试文件系统的回收系统是否正常运行
async fn test_many() {
    let rw = (true, true);
    let mut manager = VfsManager::new(3);
    manager.init_clock(Box::new(ZeroClock));
    manager.init_devalloc(Box::new(ArcDevAlloc::new()));
    manager.mount(xp(""), xp("/"), "tmpfs", 0).await.unwrap();
    let _d00 = manager.create(xp("/0"), false, rw).await.unwrap();
    let _d01 = manager.create(xp("/1"), false, rw).await.unwrap();
    let _d02 = manager.create(xp("/2"), false, rw).await.unwrap();
    let _d03 = manager.create(xp("/3"), false, rw).await.unwrap();
    {
        let _d04 = manager.create(xp("/4"), false, rw).await.unwrap();
        let _d05 = manager.create(xp("/5"), false, rw).await.unwrap();
        let _d06 = manager.create(xp("/6"), false, rw).await.unwrap();
        {
            let _d10 = manager.open(xp("/0")).await.unwrap();
            let _d11 = manager.open(xp("/1")).await.unwrap();
            let _d12 = manager.open(xp("/2")).await.unwrap();
            let _d13 = manager.open(xp("/3")).await.unwrap();
            let _d14 = manager.open(xp("/4")).await.unwrap();
            let _d15 = manager.open(xp("/5")).await.unwrap();
            let _d16 = manager.open(xp("/6")).await.unwrap();
        }
    }
    println!("begin release because the number of caches is 3");
}

async fn test_unlink() {
    let rw = (true, true);
    let mut manager = VfsManager::new(10);
    manager.init_clock(Box::new(ZeroClock));
    manager.init_devalloc(Box::new(ArcDevAlloc::new()));
    manager.mount(xp(""), xp("/"), "tmpfs", 0).await.unwrap();
    let _0 = manager.create(xp("/0"), false, rw).await.unwrap();
    manager.open(xp("/0")).await.unwrap();
    manager.unlink(xp("/0")).await.unwrap();
    manager.open(xp("/0")).await.unwrap_err();
}

async fn test_rmdir() {
    let rw = (true, true);
    let mut manager = VfsManager::new(10);
    manager.init_clock(Box::new(ZeroClock));
    manager.init_devalloc(Box::new(ArcDevAlloc::new()));
    manager.mount(xp(""), xp("/"), "tmpfs", 0).await.unwrap();
    let x = manager.create(xp("/1"), true, rw).await.unwrap();
    manager.rmdir(xp("/1")).await.unwrap();
    let _ = manager.create(xp("/2"), true, rw).await.unwrap();
    manager.rmdir(xp("/2")).await.unwrap();
    manager.create((Ok(x), "3"), false, rw).await.unwrap_err();
    let d1 = manager.create(xp("/1"), true, rw).await.unwrap();
    let _d11 = manager
        .create((Ok(d1.clone()), "1"), false, rw)
        .await
        .unwrap();
    manager.rmdir(xp("/1")).await.unwrap_err();
    manager.rmdir((Ok(d1), "")).await.unwrap_err();
}

async fn test_special() {
    let mut manager = VfsManager::new(10);
    manager.init_clock(Box::new(ZeroClock));
    manager.init_devalloc(Box::new(ArcDevAlloc::new()));
    manager.set_spec_dentry("dev".to_string());
    manager.mount(xp(""), xp("/"), "tmpfs", 0).await.unwrap();
    manager.open(xp("/dev")).await.unwrap();
}
