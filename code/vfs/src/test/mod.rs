use ftl_util::{async_tools::tiny_env, error::SysError};

use crate::{manager::BaseFn, VfsManager};

#[test]
pub fn test1() {
    let (executor, spawner) = tiny_env::new_executor_and_spawner();
    spawner.spawn(test_async());
    executor.run_debug();
}

async fn test_async() {
    let manager = VfsManager::new(10);
    manager
        .mount(xpath(""), xpath("/"), "tmpfs", 0)
        .await
        .unwrap();
    let d0 = manager.create(xpath("/0"), false).await.unwrap();
}

fn xpath(path: &str) -> (impl BaseFn, &str) {
    (|| Err(SysError::ENOENT), path)
}
