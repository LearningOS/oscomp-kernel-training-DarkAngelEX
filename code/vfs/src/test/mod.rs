mod xasync;

use ftl_util::error::SysError;

use crate::{manager::BaseFn, VfsManager};

#[test]
pub fn test1() {
    let (executor, spawner) = xasync::new_executor_and_spawner();
    spawner.spawn(test_async());
    executor.run();
}

fn xpath(path: &str) -> (impl BaseFn, &str) {
    (|| Err(SysError::ENOENT), path)
}

async fn test_async() {
    let manager = VfsManager::new(10);
    manager
        .mount(xpath(""), xpath("/"), "tmpfs", 0)
        .await
        .unwrap();
}
