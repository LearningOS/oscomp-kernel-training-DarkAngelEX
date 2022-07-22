#![allow(dead_code)]

use core::{
    cell::UnsafeCell,
    fmt,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicUsize, Ordering},
};

use crate::async_tools::SendWraper;

use super::MutexSupport;

/// 序列锁
///
/// 读者方的无锁读取, 但如果有写者将重试
///
/// seq为偶数时为无锁, 奇数为有锁, 读者会在读取后判断seq是否发生变化
pub struct SeqMutex<T: ?Sized, S: MutexSupport> {
    seq: AtomicUsize,
    _marker: PhantomData<S>,
    data: UnsafeCell<T>, // actual data
}

struct SeqMutexGuard<'a, T: ?Sized, S: MutexSupport> {
    mutex: &'a SeqMutex<T, S>,
    support_guard: S::GuardData,
}
// 禁止Mutex跨越await导致死锁或无意义阻塞
impl<'a, T: ?Sized, S: MutexSupport> !Sync for SeqMutexGuard<'a, T, S> {}
impl<'a, T: ?Sized, S: MutexSupport> !Send for SeqMutexGuard<'a, T, S> {}

unsafe impl<T: ?Sized + Send, S: MutexSupport> Sync for SeqMutex<T, S> {}
unsafe impl<T: ?Sized + Send, S: MutexSupport> Send for SeqMutex<T, S> {}

impl<T, S: MutexSupport> SeqMutex<T, S> {
    pub const fn new(user_data: T) -> Self {
        SeqMutex {
            seq: AtomicUsize::new(0),
            data: UnsafeCell::new(user_data),
            _marker: PhantomData,
        }
    }

    /// Consumes this mutex, returning the underlying data.
    pub fn into_inner(self) -> T {
        // We know statically that there are no outstanding references to
        // `self` so there's no need to lock.
        let SeqMutex { data, .. } = self;
        data.into_inner()
    }
}

impl<T: ?Sized, S: MutexSupport> SeqMutex<T, S> {
    #[inline(always)]
    pub fn get_mut(&mut self) -> &mut T {
        self.data.get_mut()
    }
    #[inline(always)]
    pub unsafe fn unsafe_get(&self) -> &T {
        &*self.data.get()
    }
    #[inline(always)]
    pub unsafe fn unsafe_get_mut(&self) -> &mut T {
        &mut *self.data.get()
    }
    /// Assume the mutex is free and get reference of value.
    ///
    /// This is only safe during initialization
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn assert_unique_get(&self) -> &mut T {
        assert!(!self.seq.load(Ordering::Relaxed) % 2 == 0);
        &mut *self.data.get()
    }
    ///
    /// 此函数运行时不会产生任何原子同步指令, 只有两次fence
    ///
    /// 读取过程中如果有写者介入会重新开始读取过程
    ///
    #[inline(always)]
    pub fn read<U>(&self, mut run: impl FnMut(&T) -> U) -> U {
        let mut seq = self.seq.load(Ordering::Acquire);
        let mut ret;
        loop {
            seq = self.wait_unlock(seq);
            ret = run(unsafe { &*self.data.get() });
            let new_seq = self.seq.load(Ordering::Acquire);
            if seq == new_seq {
                break;
            }
            seq = new_seq;
        }
        ret
    }
    #[inline(always)]
    pub fn try_read<U>(&self, mut run: impl FnMut(&T) -> U) -> Option<U> {
        let seq = self.seq.load(Ordering::Acquire);
        if seq % 2 != 0 {
            return None;
        }
        let ret = run(unsafe { &*self.data.get() });
        if seq != self.seq.load(Ordering::Acquire) {
            return None;
        }
        Some(ret)
    }
    /// Wait until the lock looks unlocked before retrying
    #[inline(always)]
    fn wait_unlock(&self, mut seq: usize) -> usize {
        let mut try_count = 0usize;
        while seq % 2 != 0 {
            try_count += 1;
            if try_count == 0x10000000 {
                panic!("Mutex: deadlock detected! try_count > {:#x}\n", try_count);
            }
            core::hint::spin_loop();
            seq = self.seq.load(Ordering::Relaxed);
        }
        seq
    }
    #[inline(always)]
    pub fn write_lock(&self) -> impl DerefMut<Target = T> + '_ {
        let mut support_guard;
        let mut seq = self.seq.load(Ordering::Relaxed);
        loop {
            seq = self.wait_unlock(seq);
            support_guard = S::before_lock();
            match self.seq.compare_exchange(
                seq,
                seq.wrapping_add(1),
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(s) => {
                    S::after_unlock(&mut support_guard);
                    seq = s
                }
            }
        }
        SeqMutexGuard {
            mutex: self,
            support_guard,
        }
    }
    #[inline(always)]
    pub unsafe fn send_write_lock(&self) -> impl DerefMut<Target = T> + Send + '_ {
        SendWraper::new(self.write_lock())
    }
    #[inline(always)]
    pub fn get_ptr(&self) -> *mut T {
        self.data.get()
    }

    /// Tries to lock the mutex. If it is already locked, it will return None. Otherwise it returns
    /// a guard within Some.
    #[inline(always)]
    pub fn try_write_lock(&self) -> Option<impl DerefMut<Target = T> + '_> {
        let seq = self.seq.load(Ordering::Relaxed);
        if seq % 2 != 0 {
            return None;
        }
        let mut support_guard = S::before_lock();
        if self
            .seq
            .compare_exchange(
                seq,
                seq.wrapping_add(1),
                Ordering::Acquire,
                Ordering::Relaxed,
            )
            .is_ok()
        {
            Some(SeqMutexGuard {
                mutex: self,
                support_guard,
            })
        } else {
            S::after_unlock(&mut support_guard);
            None
        }
    }
}

impl<T: ?Sized + fmt::Debug, S: MutexSupport> fmt::Debug for SeqMutex<T, S> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.try_write_lock() {
            Some(guard) => write!(f, "Mutex {{ data: {:?} }}", &*guard),
            None => write!(f, "Mutex {{ <locked> }}"),
        }
    }
}

impl<T: ?Sized + ~const Default, S: MutexSupport> const Default for SeqMutex<T, S> {
    fn default() -> SeqMutex<T, S> {
        SeqMutex::new(Default::default())
    }
}

impl<'a, T: ?Sized, S: MutexSupport> Deref for SeqMutexGuard<'a, T, S> {
    type Target = T;
    #[inline(always)]
    fn deref(&self) -> &T {
        unsafe { &*self.mutex.data.get() }
    }
}

impl<'a, T: ?Sized, S: MutexSupport> DerefMut for SeqMutexGuard<'a, T, S> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<'a, T: ?Sized, S: MutexSupport> Drop for SeqMutexGuard<'a, T, S> {
    /// The dropping of the MutexGuard will release the lock it was created from.
    #[inline(always)]
    fn drop(&mut self) {
        let seq = self.mutex.seq.load(Ordering::Relaxed);
        debug_assert!(seq % 2 != 0);
        self.mutex.seq.store(seq.wrapping_add(1), Ordering::Release);
        S::after_unlock(&mut self.support_guard);
    }
}
