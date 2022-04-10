use alloc::{
    boxed::Box,
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use core::{future::Future, pin::Pin};

use crate::{mutex::SpinMutex, tools::SID, xerror::SysError};

pub struct SyncTask(Pin<Box<dyn Future<Output = Result<(), SysError>> + Send + 'static>>);

impl SyncTask {
    pub fn new(task: impl Future<Output = Result<(), SysError>> + Send + 'static) -> Self {
        Self(Box::pin(task))
    }
    pub async fn run(self) -> Result<(), SysError> {
        self.0.await
    }
}

/// 此管理器仅负责磁盘块同步 不要向此管理器提交读磁盘task!
///
/// 当一个扇区的写任务未结束时, 再次获得写请求时不会立刻发送任务 保证一个扇区不会同时运行在多个任务中
///
/// 对一个扇区的新task将覆盖旧task
///
/// 为了确保顺序的绝对正确, 应该在持有扇区锁的情况下加入此任务
///
/// 此管理器发送任务的过程中不需要再次获取扇区锁
pub struct SyncManager {
    running: BTreeSet<SID>,
    pending: BTreeMap<SID, SyncTask>,
    spawn_handler: Option<Box<dyn FnMut(SyncTask)>>,
}

impl SyncManager {
    pub const fn new() -> Self {
        Self {
            running: BTreeSet::new(),
            pending: BTreeMap::new(),
            spawn_handler: None,
        }
    }
    /// 任务将在未来执行完成
    pub fn no_pending(&self) -> bool {
        self.pending.is_empty()
    }
    /// 所有写请求都执行完成
    pub fn idle(&self) -> bool {
        self.pending.is_empty() && self.running.is_empty()
    }
    pub fn insert(this: &Arc<SpinMutex<Self>>, sid: SID, task: SyncTask) {
        let mut lock = this.lock();
        if lock.running.contains(&sid) {
            let _ = lock.pending.insert(sid, task);
        } else {
            lock.running.insert(sid);
            let callback_task = Self::task_add_callback(this.clone(), sid, task);
            lock.spawn_handler.as_mut().unwrap()(callback_task);
        }
    }
    pub fn insert_iter(this: &Arc<SpinMutex<Self>>, iter: impl Iterator<Item = (SID, SyncTask)>) {
        let mut lock = this.lock();
        for (sid, task) in iter {
            if lock.running.contains(&sid) {
                let _ = lock.pending.insert(sid, task);
            } else {
                lock.running.insert(sid);
                let callback_task = Self::task_add_callback(this.clone(), sid, task);
                lock.spawn_handler.as_mut().unwrap()(callback_task);
            }
        }
    }
    fn task_add_callback(this: Arc<SpinMutex<Self>>, sid: SID, task: SyncTask) -> SyncTask {
        SyncTask::new(async move {
            let ret = task.run().await;
            Self::call_back(this, sid);
            ret
        })
    }
    fn call_back(this: Arc<SpinMutex<Self>>, sid: SID) {
        let mut lock = this.lock();
        if !lock.running.remove(&sid) {
            panic!();
        }
        if let Some(task) = lock.pending.remove(&sid) {
            let callback_task = Self::task_add_callback(this.clone(), sid, task);
            lock.spawn_handler.as_mut().unwrap()(callback_task);
        }
    }
}
