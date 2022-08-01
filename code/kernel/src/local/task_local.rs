use alloc::sync::Arc;

use crate::{
    memory::PageTable, process::thread::Thread, tools::container::sync_unsafe_cell::SyncUnsafeCell,
};

use super::always_local::AlwaysLocal;

/// `TaskLocal`是用户线程的控制结构, 通过它可以在代码的任意位置访问到
/// 当前的线程和页表, 以及线程自身的`AlwaysLocal`.
///
/// 内核线程和调度状态不存在`TaskLocal`,因此并不是任意时刻都可以获取到
/// `TaskLocal`, 因此`TaskLocal`不可以用于内核中断上下文, 中断上下文
/// 只可以使用`AlwaysLocal`.
///
/// `TaskLocal`不是一个轻量级的结构, 以值的方式切换可行但效率很低,
/// FTL OS通过指针交换方式来快速地在不同线程之间切换`TaskLocal`.
pub struct TaskLocal {
    pub always_local: AlwaysLocal,
    pub thread: Arc<Thread>,
    // 进程改变页表时需要同步到这里，更新回OutermostFuture
    pub page_table: Arc<SyncUnsafeCell<PageTable>>,
}
