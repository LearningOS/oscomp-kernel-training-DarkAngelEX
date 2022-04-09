#![allow(dead_code)]
use alloc::collections::LinkedList;

use core::{
    cell::UnsafeCell,
    future::Future,
    ops::{Deref, DerefMut},
    pin::Pin,
    task::{Context, Poll, Waker},
};

use crate::mutex::{Mutex, MutexSupport};

pub struct RwSleepMutex<T, S: MutexSupport> {
    inner: Mutex<RwSleepMutexSupport, S>,
    data: UnsafeCell<T>, // actual data
}

unsafe impl<T, S: MutexSupport> Sync for RwSleepMutex<T, S> {}
unsafe impl<T, S: MutexSupport> Send for RwSleepMutex<T, S> {}

pub struct SleepMutexGuard<'a, T, S: MutexSupport, const UNIQUE: bool> {
    mutex: &'a RwSleepMutex<T, S>,
}

unsafe impl<'a, T, S: MutexSupport, const UNIQUE: bool> Sync for SleepMutexGuard<'a, T, S, UNIQUE> {}
unsafe impl<'a, T, S: MutexSupport, const UNIQUE: bool> Send for SleepMutexGuard<'a, T, S, UNIQUE> {}

impl<T, S: MutexSupport> RwSleepMutex<T, S> {
    pub const fn new(user_data: T) -> Self {
        Self {
            inner: Mutex::new(RwSleepMutexSupport::new()),
            data: UnsafeCell::new(user_data),
        }
    }
    /// 睡眠锁将交替解锁共享任务和排他任务 不保证共享锁和排他锁的解锁顺序
    pub async fn shared_lock(&self) -> impl Deref<Target = T> + Send + Sync + '_ {
        type SharedSleepMutexFuture<'a, T, S> = SleepMutexFuture<'a, T, S, false>;
        SharedSleepMutexFuture::new(self).await
    }
    /// 睡眠锁将交替解锁共享任务和排他任务 不保证共享锁和排他锁的解锁顺序
    ///
    /// 对排他锁的解锁严格按照提交顺序进行
    pub async fn unique_lock(&self) -> impl DerefMut<Target = T> + Send + Sync + '_ {
        type UniqueSleepMutexFuture<'a, T, S> = SleepMutexFuture<'a, T, S, true>;
        UniqueSleepMutexFuture::new(self).await
    }
}
impl<'a, T, S: MutexSupport, const UNIQUE: bool> Deref for SleepMutexGuard<'a, T, S, UNIQUE> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.mutex.data.get() }
    }
}
impl<'a, T, S: MutexSupport, const UNIQUE: bool> DerefMut for SleepMutexGuard<'a, T, S, UNIQUE> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<'a, T, S: MutexSupport, const UNIQUE: bool> Drop for SleepMutexGuard<'a, T, S, UNIQUE> {
    fn drop(&mut self) {
        stack_trace!();
        if UNIQUE {
            let list = self.mutex.inner.lock().after_unique_lock();
            list.into_iter().for_each(|(_, waker)| waker.wake());
        } else {
            let w = self.mutex.inner.lock().after_shared_lock();
            if let Some(w) = w {
                w.wake();
            }
        };
    }
}

struct SleepMutexFuture<'a, T, S: MutexSupport, const UNIQUE: bool> {
    mutex: &'a RwSleepMutex<T, S>,
    id: usize, // 0 means need alloc then.
}

impl<'a, T, S: MutexSupport, const UNIQUE: bool> SleepMutexFuture<'a, T, S, UNIQUE> {
    pub fn new(mutex: &'a RwSleepMutex<T, S>) -> Self {
        SleepMutexFuture { mutex, id: 0 }
    }
}

impl<'a, T, S: MutexSupport, const UNIQUE: bool> Future for SleepMutexFuture<'a, T, S, UNIQUE> {
    type Output = SleepMutexGuard<'a, T, S, UNIQUE>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        stack_trace!();
        let mut mutex = self.mutex.inner.lock();

        match match UNIQUE {
            true => mutex.unique_lock(&mut self.id, || cx.waker().clone()),
            false => mutex.shared_lock(&mut self.id, || cx.waker().clone()),
        } {
            true => Poll::Ready(SleepMutexGuard { mutex: self.mutex }),
            false => Poll::Pending,
        }
    }
}
#[derive(Debug)]
enum Slot {
    /// 未上锁 全部队列为空
    Any,
    /// 处于排他锁
    Locked,
    /// 未上锁 等待发射ID等于值的排他任务 unique队列可能为空
    Unique(usize),
    /// 处于共享锁且无处于等待的排他锁 新加入的共享任务直接发射
    Shared(usize, usize),
    /// 处于共享锁 有正在等待的排他任务 不允许新共享任务加入
    SharedPending(usize, usize),
}

/// 交替发射全部共享任务和排他任务 排他任务严格按顺序发射 共享任务发射组按顺序发射
struct RwSleepMutexSupport {
    shared: LinkedList<(usize, Waker)>,
    unique: LinkedList<(usize, Waker)>,
    slot: Slot,
    id_alloc: usize,
}

impl RwSleepMutexSupport {
    pub const fn new() -> Self {
        RwSleepMutexSupport {
            shared: LinkedList::new(),
            unique: LinkedList::new(),
            slot: Slot::Any,
            id_alloc: 1,
        }
    }
    pub fn after_unique_lock(&mut self) -> LinkedList<(usize, Waker)> {
        debug_assert!(matches!(self.slot, Slot::Locked));
        // 发射全部共享任务
        if let Some((id, _)) = self.shared.back() {
            self.slot = match self.unique.is_empty() {
                true => Slot::Shared,
                false => Slot::SharedPending,
            }(*id, self.shared.len());
            return core::mem::take(&mut self.shared);
        }
        // 发射一个排他任务
        self.slot = match self.unique.pop_front() {
            Some((id, waker)) => {
                waker.wake();
                Slot::Unique(id)
            }
            None => Slot::Any,
        };
        return LinkedList::new();
    }
    pub fn after_shared_lock(&mut self) -> Option<Waker> {
        match &mut self.slot {
            Slot::Shared(_, num) | Slot::SharedPending(_, num) => {
                let old = *num;
                *num = old - 1;
                match old {
                    #[cfg(debug_assertions)]
                    0 => panic!(),
                    #[cfg(not(debug_assertions))]
                    0 => unsafe { core::hint::unreachable_unchecked() },
                    1 => (),
                    _ => return None,
                }
            }
            e => panic!("{:?}", e),
        }
        match self.slot {
            Slot::Shared(_, 0) => {
                debug_assert!(self.shared.is_empty());
                debug_assert!(self.unique.is_empty());
                self.slot = Slot::Any;
                None
            }
            Slot::SharedPending(_, 0) => {
                let (id, waker) = self.unique.pop_front().unwrap();
                self.slot = Slot::Unique(id);
                Some(waker)
            }
            #[cfg(debug_assertions)]
            _ => unreachable!(),
            #[cfg(not(debug_assertions))]
            _ => unsafe { core::hint::unreachable_unchecked() },
        }
    }
    /// 返回 *id == 0
    fn try_alloc_id(&mut self, id: &mut usize) -> bool {
        if *id != 0 {
            return false;
        }
        *id = self.id_alloc;
        debug_assert!(*id != usize::MAX);
        self.id_alloc += 1;
        true
    }
    fn shared_lock_first(&mut self, id: usize, waker_fn: impl FnOnce() -> Waker) -> bool {
        debug_assert!((1..self.id_alloc).contains(&id));
        match self.slot {
            Slot::Any => {
                self.slot = Slot::Shared(id, 1);
                true
            }
            Slot::Locked | Slot::Unique(_) => {
                self.shared.push_back((id, waker_fn()));
                false
            }
            Slot::Shared(last_id, num) => {
                debug_assert!(last_id + 1 == id);
                debug_assert!(self.shared.is_empty());
                debug_assert!(self.unique.is_empty());
                self.slot = Slot::Shared(id, num + 1);
                true
            }
            Slot::SharedPending(last_id, _num) => {
                debug_assert!(last_id < id);
                debug_assert!(!self.unique.is_empty());
                self.shared.push_back((id, waker_fn()));
                false
            }
        }
    }
    fn unique_lock_first(&mut self, id: usize, waker_fn: impl FnOnce() -> Waker) -> bool {
        match self.slot {
            Slot::Any => {
                self.slot = Slot::Locked;
                true
            }
            Slot::Shared(id, num) => {
                self.slot = Slot::SharedPending(id, num);
                self.unique.push_back((id, waker_fn()));
                false
            }
            Slot::Locked | Slot::SharedPending(_, _) | Slot::Unique(_) => {
                self.unique.push_back((id, waker_fn()));
                false
            }
        }
    }
    pub fn shared_lock(&mut self, rid: &mut usize, waker_fn: impl FnOnce() -> Waker) -> bool {
        if self.try_alloc_id(rid) {
            return self.shared_lock_first(*rid, waker_fn);
        }
        let id = *rid;
        match self.slot {
            Slot::Any => unreachable!(),
            // 排他锁释放后状态可能转变为Shared
            Slot::Shared(max_id, _) => {
                debug_assert!(max_id >= id);
                true
            }
            Slot::SharedPending(max_id, _) => id <= max_id,
            Slot::Locked | Slot::Unique(_) => false,
        }
    }
    pub fn unique_lock(&mut self, rid: &mut usize, waker_fn: impl FnOnce() -> Waker) -> bool {
        if self.try_alloc_id(rid) {
            return self.unique_lock_first(*rid, waker_fn);
        }
        let id = *rid;
        match self.slot {
            Slot::Any | Slot::Shared(_, _) => unreachable!(),
            Slot::Unique(xid) if xid == id => true,
            _ => false,
        }
    }
}
