pub mod sync_loop;

use alloc::{
    boxed::Box,
    collections::{BTreeMap, BTreeSet, LinkedList},
    sync::Arc,
};
use core::{future::Future, pin::Pin};

use crate::{
    mutex::{Mutex, MutexSupport},
    tools::SID,
};

pub struct SyncTask(Pin<Box<dyn Future<Output = Result<(), ()>> + Send + 'static>>);

impl SyncTask {
    pub fn new(task: impl Future<Output = Result<(), ()>> + Send + 'static) -> Self {
        Self(Box::pin(task))
    }
    pub async fn run(self) -> Result<(), ()> {
        self.0.await
    }
}

/// 此管理器仅负责磁盘块同步 不要向此管理器提交读磁盘task! (读任务由进程task进行)
///
/// 对一个扇区的新task将覆盖旧task 因此读task将覆盖写task
///
/// 为了确保顺序的绝对正确, 应该在持有扇区锁的情况下加入此任务
///
/// 保证一个扇区不会同时运行在多个任务中
pub struct SyncManager {
    closed: bool,
    tasks_running: BTreeSet<SID>,
    tasks_pending: BTreeMap<SID, SyncTask>,
    tasks_ready: BTreeMap<SID, SyncTask>,
    current: SID,
}

impl SyncManager {
    pub const fn new() -> Self {
        Self {
            closed: false,
            tasks_running: BTreeSet::new(),
            tasks_pending: BTreeMap::new(),
            tasks_ready: BTreeMap::new(),
            current: SID(0),
        }
    }
    pub fn close(&mut self) {
        self.closed = true;
    }
    pub fn closed(&self) -> bool {
        self.closed
    }
    pub fn no_task_will_running(&self) -> bool {
        self.tasks_pending.is_empty() && self.tasks_ready.is_empty()
    }
    pub fn no_task(&self) -> bool {
        self.tasks_pending.is_empty()
            && self.tasks_ready.is_empty()
            && self.tasks_running.is_empty()
    }
    pub fn insert(&mut self, sid: SID, task: SyncTask) {
        assert!(!self.closed);
        if self.tasks_running.contains(&sid) {
            let _ = self.tasks_pending.insert(sid, task);
        } else {
            let _ = self.tasks_ready.insert(sid, task);
        }
    }
    pub fn insert_list(&mut self, list: LinkedList<(SID, SyncTask)>) {
        assert!(!self.closed);
        for (sid, task) in list.into_iter() {
            self.insert(sid, task);
        }
    }
    pub fn fetch<S: MutexSupport>(this: &Arc<Mutex<Self, S>>) -> Option<SyncTask> {
        let ptr = this.clone();
        this.lock().fetch_impl(
            move |a, id| a.task_call_back(id),
            move |f, id| f(&mut *ptr.lock(), id),
        )
    }
    fn fetch_impl<F1, F2>(&mut self, callback: F1, self_run: F2) -> Option<SyncTask>
    where
        F1: FnOnce(&mut Self, SID) + Send + 'static,
        F2: FnOnce(F1, SID) + Send + 'static,
    {
        if self.tasks_ready.is_empty() {
            return None;
        }
        let (id, task) = if let Some((&id, _)) = self.tasks_ready.range(self.current..).next() {
            self.current = SID(id.0 + 1);
            (id, self.tasks_ready.remove(&id).unwrap())
        } else {
            let (id, task) = self.tasks_ready.pop_first().unwrap();
            self.current = SID(id.0 + 1);
            (id, task)
        };
        Some(SyncTask::new(async move {
            let ret = task.run().await;
            self_run(callback, id);
            ret
        }))
    }
    fn task_call_back(&mut self, sid: SID) {
        if !self.tasks_running.remove(&sid) {
            panic!();
        }
        if let Some(task) = self.tasks_pending.remove(&sid) {
            self.tasks_ready.try_insert(sid, task).ok().unwrap();
        }
    }
}
