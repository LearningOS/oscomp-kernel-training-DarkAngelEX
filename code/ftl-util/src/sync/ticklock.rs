#![allow(dead_code)]

use core::{
    cell::{SyncUnsafeCell, UnsafeCell},
    marker::PhantomData,
    ops::{Deref, DerefMut},
    sync::atomic::{self, AtomicU32, AtomicU64, Ordering},
};

use crate::async_tools::SendWraper;

use super::MutexSupport;

#[derive(Clone, Copy)]
struct TC {
    tick: u32,
    cur: u32,
}

union TCU {
    tc: TC,
    val: u64,
}
impl TCU {
    pub fn new_tc(tc: TC) -> Self {
        Self { tc }
    }
    pub fn new_val(val: u64) -> Self {
        Self { val }
    }
    pub fn tc(self) -> TC {
        unsafe { self.tc }
    }
    pub fn val(self) -> u64 {
        unsafe { self.val }
    }
}

struct AtomicTCU(SyncUnsafeCell<TCU>);

impl AtomicTCU {
    pub const fn new() -> Self {
        Self(SyncUnsafeCell::new(TCU { val: 0 }))
    }
    pub fn tick(&self) -> &AtomicU32 {
        unsafe { core::mem::transmute(&(*self.0.get()).tc.tick) }
    }
    pub fn cur(&self) -> &AtomicU32 {
        unsafe { core::mem::transmute(&(*self.0.get()).tc.cur) }
    }
    fn val(&self) -> &AtomicU64 {
        unsafe { core::mem::transmute(&(*self.0.get()).val) }
    }
    pub fn load(&self, order: Ordering) -> TC {
        unsafe { TCU::new_val(self.val().load(order)).tc }
    }
    pub fn cas(&self, old: TC, new: TC, order: Ordering) -> Result<(), TC> {
        let old = TCU::new_tc(old).val();
        let new = TCU::new_tc(new).val();
        match self
            .val()
            .compare_exchange(old, new, order, Ordering::Relaxed)
        {
            Ok(_) => Ok(()),
            Err(val) => Err(TCU::new_val(val).tc()),
        }
    }
}

pub struct TickLock<T: ?Sized, S: MutexSupport> {
    atc: AtomicTCU,
    _marker: PhantomData<S>,
    data: UnsafeCell<T>, // actual data
}

struct TickGuard<'a, T: ?Sized, S: MutexSupport> {
    mutex: &'a TickLock<T, S>,
    tick: u32,
    support_guard: S::GuardData,
}
// 禁止Mutex跨越await导致死锁或无意义阻塞
impl<'a, T: ?Sized, S: MutexSupport> !Sync for TickGuard<'a, T, S> {}
impl<'a, T: ?Sized, S: MutexSupport> !Send for TickGuard<'a, T, S> {}

unsafe impl<T: ?Sized + Send, S: MutexSupport> Sync for TickLock<T, S> {}
unsafe impl<T: ?Sized + Send, S: MutexSupport> Send for TickLock<T, S> {}

impl<T, S: MutexSupport> TickLock<T, S> {
    pub const fn new(user_data: T) -> Self {
        TickLock {
            atc: AtomicTCU::new(),
            data: UnsafeCell::new(user_data),
            _marker: PhantomData,
        }
    }

    /// Consumes this mutex, returning the underlying data.
    pub fn into_inner(self) -> T {
        // We know statically that there are no outstanding references to
        // `self` so there's no need to lock.
        let TickLock { data, .. } = self;
        data.into_inner()
    }
}

impl<T: ?Sized, S: MutexSupport> TickLock<T, S> {
    #[inline(always)]
    pub fn get_mut(&mut self) -> &mut T {
        self.data.get_mut()
    }
    /// # Safety
    ///
    /// 用户保证内部读取的安全性
    #[inline(always)]
    pub unsafe fn unsafe_get(&self) -> &T {
        &*self.data.get()
    }
    /// # Safety
    ///
    /// 用户保证内部读取的安全性
    #[allow(clippy::mut_from_ref)]
    #[inline(always)]
    pub unsafe fn unsafe_get_mut(&self) -> &mut T {
        &mut *self.data.get()
    }
    /// Wait until the lock looks unlocked before retrying
    #[inline(always)]
    fn wait_tick(&self, tick: u32) {
        let mut try_count = 0usize;
        while self.atc.cur().load(Ordering::Relaxed) != tick {
            core::hint::spin_loop();
            try_count += 1;
            if try_count == 0x10000000 {
                panic!("Mutex: deadlock detected! try_count > {:#x}\n", try_count);
            }
        }
    }
    #[inline(always)]
    pub fn lock(&self) -> impl DerefMut<Target = T> + '_ {
        let support_guard = S::before_lock();
        let tick = self.atc.tick().fetch_add(1, Ordering::Relaxed);
        self.wait_tick(tick);
        atomic::fence(Ordering::Acquire);
        TickGuard {
            mutex: self,
            tick,
            support_guard,
        }
    }
    /// # Safety
    ///
    /// 需要保证持有锁时不发生上下文切换
    #[inline(always)]
    pub unsafe fn send_lock(&self) -> impl DerefMut<Target = T> + Send + '_ {
        SendWraper::new(self.lock())
    }
    #[inline(always)]
    pub fn get_ptr(&self) -> *mut T {
        self.data.get()
    }

    /// Tries to lock the mutex. If it is already locked, it will return None. Otherwise it returns
    /// a guard within Some.
    #[inline(always)]
    pub fn try_lock(&self) -> Option<impl DerefMut<Target = T> + '_> {
        let tc = self.atc.load(Ordering::Relaxed);
        if tc.tick != tc.cur {
            return None;
        }
        let tick = tc.tick;
        let mut new = tc;
        new.tick = tick.wrapping_add(1);
        let mut support_guard = S::before_lock();
        if self.atc.cas(tc, new, Ordering::Acquire).is_ok() {
            Some(TickGuard {
                mutex: self,
                tick,
                support_guard,
            })
        } else {
            S::after_unlock(&mut support_guard);
            None
        }
    }
}

impl<T: ?Sized + ~const Default, S: MutexSupport> const Default for TickLock<T, S> {
    fn default() -> TickLock<T, S> {
        TickLock::new(Default::default())
    }
}

impl<'a, T: ?Sized, S: MutexSupport> Deref for TickGuard<'a, T, S> {
    type Target = T;
    #[inline(always)]
    fn deref(&self) -> &T {
        unsafe { &*self.mutex.data.get() }
    }
}

impl<'a, T: ?Sized, S: MutexSupport> DerefMut for TickGuard<'a, T, S> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<'a, T: ?Sized, S: MutexSupport> Drop for TickGuard<'a, T, S> {
    /// The dropping of the MutexGuard will release the lock it was created from.
    #[inline(always)]
    fn drop(&mut self) {
        debug_assert_eq!(self.tick, self.mutex.atc.cur().load(Ordering::Relaxed));
        let next = self.tick.wrapping_add(1);
        self.mutex.atc.cur().store(next, Ordering::Release);
        S::after_unlock(&mut self.support_guard);
    }
}
