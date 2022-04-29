use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

use alloc::{collections::VecDeque, sync::Arc};

use crate::tools::AID;

use super::spin_mutex::SpinMutex;

/// 此信号量生成的guard通过Arc维护,可以发送至'static闭包
pub struct Semaphore {
    inner: Arc<SpinMutex<SemaphoreInner>>,
}

struct SemaphoreInner {
    pub alloc_id: AID,
    pub allow_id: AID,
    pub max_size: usize,
    pub cur_size: usize,
    pub queue: VecDeque<(AID, usize, Waker)>,
}

impl Drop for SemaphoreInner {
    fn drop(&mut self) {
        debug_assert!(self.cur_size == 0);
    }
}

/// 原子获取1个信号量
pub struct SemaphoreGuard {
    inner: MultiplySemaphore,
}

/// 原子获取多个信号量
pub struct MultiplySemaphore {
    val: usize,
    ptr: Arc<SpinMutex<SemaphoreInner>>,
}

impl Drop for SemaphoreGuard {
    fn drop(&mut self) {
        debug_assert!(self.inner.val == 1);
    }
}
impl Drop for MultiplySemaphore {
    fn drop(&mut self) {
        if self.val != 0 {
            let mut inner = self.ptr.lock();
            inner.cur_size -= self.val;
            inner.release_task();
        }
    }
}
impl SemaphoreGuard {
    pub fn into_multiply(self) -> MultiplySemaphore {
        unsafe { core::mem::transmute(self) }
    }
}
impl MultiplySemaphore {
    pub fn val(&self) -> usize {
        self.val
    }
    pub fn try_take(&mut self) -> Option<SemaphoreGuard> {
        if self.val == 0 {
            return None;
        }
        self.val -= 1;
        Some(SemaphoreGuard {
            inner: Self {
                val: 1,
                ptr: self.ptr.clone(),
            },
        })
    }
}

impl SemaphoreInner {
    fn release_task(&mut self) {
        if self.cur_size < self.max_size && !self.queue.is_empty() {
            while let Some((id, v, _w)) = self.queue.front() {
                debug_assert!(*id < self.alloc_id);
                debug_assert!(self.allow_id.0 + 1 == id.0);
                if self.cur_size + *v > self.max_size {
                    break;
                }
                self.cur_size += *v;
                self.alloc_id = *id;
                self.queue.pop_front().unwrap().2.wake();
            }
            if self.queue.len() < self.queue.capacity() / 2 {
                self.queue.shrink_to_fit();
            }
        }
    }
}

impl Semaphore {
    pub fn new(n: usize) -> Self {
        Self {
            inner: Arc::new(SpinMutex::new(SemaphoreInner {
                alloc_id: AID(1),
                allow_id: AID(1),
                max_size: n,
                cur_size: 0,
                queue: VecDeque::new(),
            })),
        }
    }
    pub fn set_max(&self, n: usize) {
        let mut inner = self.inner.lock();
        inner.max_size = n;
        inner.release_task();
    }
    /// 最大信号量
    pub fn max(&self) -> usize {
        unsafe { self.inner.unsafe_get().max_size }
    }
    /// 获取一个信号量
    pub async fn take(&self) -> SemaphoreGuard {
        SemaphoreGuard {
            inner: self.take_n(1).await,
        }
    }
    /// 获取一个信号量
    pub async fn take_n(&self, n: usize) -> MultiplySemaphore {
        SemaphoreFuture {
            val: n,
            aid: AID(0),
            sem: self,
        }
        .await
    }
}

struct SemaphoreFuture<'a> {
    val: usize,
    aid: AID,
    sem: &'a Semaphore,
}

impl<'a> Future for SemaphoreFuture<'a> {
    type Output = MultiplySemaphore;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // 此无锁释放操作由allow_id严格递增保证
        // 验证失败不能证明无法发射, 可能缓存数据未到达
        let val = self.val;
        if !self.aid.is_zero() && self.aid < unsafe { self.sem.inner.unsafe_get().allow_id } {
            return Poll::Ready(MultiplySemaphore {
                val,
                ptr: self.sem.inner.clone(),
            });
        }
        let mut inner = self.sem.inner.lock();
        if self.aid.is_zero() {
            if inner.cur_size + val <= inner.max_size {
                inner.cur_size += val;
                return Poll::Ready(MultiplySemaphore {
                    val,
                    ptr: self.sem.inner.clone(),
                });
            }
            let aid = inner.alloc_id.step();
            debug_assert!(aid > inner.allow_id);
            self.get_mut().aid = aid;
            inner.queue.push_back((aid, val, cx.waker().clone()));
            return Poll::Pending;
        }
        if self.aid > inner.allow_id {
            return Poll::Pending;
        }
        // 信号量的增加在 release_task 函数中进行
        Poll::Ready(MultiplySemaphore {
            val,
            ptr: self.sem.inner.clone(),
        })
    }
}
