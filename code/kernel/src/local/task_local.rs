use alloc::sync::Arc;

use crate::{
    memory::PageTable, process::thread::Thread, tools::container::sync_unsafe_cell::SyncUnsafeCell,
};

use super::always_local::AlwaysLocal;

/// 通过指针交换方式快速切换
///
/// 包含线程独立的信息
pub struct TaskLocal {
    pub always_local: AlwaysLocal,
    pub thread: Arc<Thread>,
    // 进程改变页表时需要同步到这里，更新回OutermostFuture
    pub page_table: Arc<SyncUnsafeCell<PageTable>>,
}
