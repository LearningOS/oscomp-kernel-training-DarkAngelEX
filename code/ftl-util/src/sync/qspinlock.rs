use core::{
    cell::SyncUnsafeCell,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    ptr::NonNull,
    sync::atomic::{self, AtomicU16, AtomicU32, AtomicU8, Ordering},
};

use crate::{local::ftl_local, MAX_CPU};

use super::MutexSupport;

const MAX_NEST: usize = 4;

#[repr(C)]
#[derive(Clone, Copy)]
struct LPT {
    locked: u8,
    pending: u8,
    tail: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
union QVal {
    val: u32,
    lpt: LPT,
    locked_pending: u16,
}
impl QVal {
    #[inline(always)]
    fn new((tail, pending, locked): (u16, u8, u8)) -> Self {
        Self {
            lpt: LPT {
                locked,
                pending,
                tail,
            },
        }
    }
    #[inline(always)]
    fn val(self) -> u32 {
        unsafe { self.val }
    }
    #[inline(always)]
    fn locked(self) -> u8 {
        unsafe { self.lpt.locked }
    }
    // #[inline(always)]
    // fn pending(self) -> u8 {
    //     unsafe { self.lpt.pending }
    // }
    #[inline(always)]
    fn locked_pending(self) -> u16 {
        unsafe { self.locked_pending }
    }
    #[inline(always)]
    fn tail(self) -> u16 {
        unsafe { self.lpt.tail }
    }
    fn mask_locked(mut self) -> Self {
        self.lpt.locked = 0;
        self
    }
}

struct QAtomicVal(SyncUnsafeCell<QVal>);

impl QAtomicVal {
    pub const fn new() -> Self {
        Self(SyncUnsafeCell::new(QVal { val: 0 }))
    }
    #[inline(always)]
    fn as_val(&self) -> &AtomicU32 {
        unsafe { core::mem::transmute(self) }
    }
    #[inline(always)]
    fn as_locked_pending(&self) -> &AtomicU16 {
        unsafe { core::mem::transmute(&(*self.0.get()).locked_pending) }
    }
    #[inline(always)]
    fn as_locked(&self) -> &AtomicU8 {
        unsafe { core::mem::transmute(&(*self.0.get()).lpt.locked) }
    }
    // #[inline(always)]
    // fn as_pending(&self) -> &AtomicU8 {
    //     unsafe { core::mem::transmute(&(*self.0.get()).lpt.pending) }
    // }
    #[inline(always)]
    fn as_tail(&self) -> &AtomicU16 {
        unsafe { core::mem::transmute(&(*self.0.get()).lpt.tail) }
    }
    #[inline(always)]
    fn load(&self, order: Ordering) -> QVal {
        QVal {
            val: self.as_val().load(order),
        }
    }
    #[inline(always)]
    fn cas_val(&self, cur: QVal, new: QVal, order: Ordering) -> Result<QVal, QVal> {
        unsafe {
            match self
                .as_val()
                .compare_exchange(cur.val, new.val, order, Ordering::Relaxed)
            {
                Ok(val) => Ok(QVal { val }),
                Err(val) => Err(QVal { val }),
            }
        }
    }
}

struct QNode {
    next: Option<NonNull<QNode>>,
    locked: u32,
    count: u32, // 自旋锁嵌套次数
}
impl QNode {
    const EMPTY: Self = Self::new();
    const fn new() -> Self {
        Self {
            next: None,
            locked: 0,
            count: 0,
        }
    }
}

#[repr(align(64))]
struct PerCPUMCS([QNode; MAX_NEST]);
impl PerCPUMCS {
    const EMPTY: Self = Self([QNode::EMPTY; MAX_NEST]);
}
static mut PER_CPU_NODES: [PerCPUMCS; MAX_CPU] = [PerCPUMCS::EMPTY; MAX_CPU];

fn current_mcs(cpuid: usize) -> &'static mut PerCPUMCS {
    debug_assert!(cpuid < MAX_CPU);
    unsafe { PER_CPU_NODES.get_unchecked_mut(cpuid) }
}

pub struct QSpinLock<T: ?Sized, S: MutexSupport> {
    qval: QAtomicVal,
    _marker: PhantomData<S>,
    data: SyncUnsafeCell<T>,
}

struct MutexGuard<'a, T: ?Sized, S: MutexSupport> {
    mutex: &'a QSpinLock<T, S>,
    support_guard: S::GuardData,
}
// 禁止Mutex跨越await导致死锁或无意义阻塞
impl<'a, T: ?Sized, S: MutexSupport> !Sync for MutexGuard<'a, T, S> {}
impl<'a, T: ?Sized, S: MutexSupport> !Send for MutexGuard<'a, T, S> {}

unsafe impl<T: ?Sized + Send, S: MutexSupport> Sync for QSpinLock<T, S> {}
unsafe impl<T: ?Sized + Send, S: MutexSupport> Send for QSpinLock<T, S> {}

impl<T, S: MutexSupport> QSpinLock<T, S> {
    pub const fn new(data: T) -> Self {
        QSpinLock {
            qval: QAtomicVal::new(),
            _marker: PhantomData,
            data: SyncUnsafeCell::new(data),
        }
    }
}

impl<T: ?Sized, S: MutexSupport> QSpinLock<T, S> {
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
    #[inline(always)]
    pub fn lock(&self) -> impl DerefMut<Target = T> + '_ {
        let support_guard = S::before_lock();
        if let Err(qval) = self.try_obtain_lock() {
            self.obtain_lock(qval);
        }
        MutexGuard {
            mutex: self,
            support_guard,
        }
    }
    #[inline(always)]
    fn try_obtain_lock(&self) -> Result<(), QVal> {
        let qval = self.qval.load(Ordering::Relaxed);
        if qval.val() == QVal::new((0, 0, 0)).val() {
            if self
                .qval
                .cas_val(qval, QVal::new((0, 0, 1)), Ordering::Acquire)
                .is_ok()
            {
                return Ok(());
            }
        }
        Err(qval)
    }
    fn obtain_lock(&self, mut qval: QVal) {
        loop {
            // 第一个核心获取锁
            if qval.val() == QVal::new((0, 0, 0)).val() {
                match self
                    .qval
                    .cas_val(qval, QVal::new((0, 0, 1)), Ordering::Acquire)
                {
                    Ok(_) => return,
                    Err(v) => {
                        qval = v;
                        continue;
                    }
                }
            }
            // 第二个核心获取锁
            if qval.val() == QVal::new((0, 0, 1)).val() {
                match self
                    .qval
                    .cas_val(qval, QVal::new((0, 1, 1)), Ordering::Relaxed)
                {
                    Ok(_) => return self.second_wait(),
                    Err(v) => {
                        qval = v;
                        continue;
                    }
                }
            }
            // 第N个核心获取锁
            let cpuid = ftl_local().cpuid();
            let cpu_nodes = current_mcs(cpuid);
            let idx = cpu_nodes.0[0].count as usize;
            debug_assert!(idx < MAX_NEST);
            cpu_nodes.0[0].count += 1;
            let node = &mut cpu_nodes.0[idx];
            *node = QNode::EMPTY;
            let tail = Self::encode_tail(cpuid, idx);
            if qval.mask_locked().val() == QVal::new((0, 1, 0)).val() {
                if qval.locked() == 0 {
                    core::hint::spin_loop();
                    qval = self.qval.load(Ordering::Relaxed);
                    cpu_nodes.0[0].count -= 1;
                    continue;
                }
                match self
                    .qval
                    .cas_val(qval, QVal::new((tail, 1, 1)), Ordering::Relaxed)
                {
                    Ok(_) => {
                        self.third_wait(node, tail);
                        cpu_nodes.0[0].count -= 1;
                        return;
                    }
                    Err(v) => {
                        qval = v;
                        cpu_nodes.0[0].count -= 1;
                        continue;
                    }
                }
            }
            match self.qval.as_tail().compare_exchange(
                qval.tail(),
                tail,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    self.last_wait(qval.tail(), node, tail);
                    cpu_nodes.0[0].count -= 1;
                    return;
                }
                Err(_) => {
                    qval = self.qval.load(Ordering::Relaxed);
                    cpu_nodes.0[0].count -= 1;
                    continue;
                }
            }
        }
    }
    #[inline(always)]
    fn encode_tail(cpuid: usize, idx: usize) -> u16 {
        let v = (cpuid + 1) * MAX_NEST + idx;
        debug_assert!(v < u16::MAX as usize);
        v as u16
    }
    #[inline(always)]
    fn decode_tail(tail: u16) -> &'static mut QNode {
        let tail = tail as usize;
        let cpu = tail / MAX_NEST - 1;
        let idx = tail % MAX_NEST;
        debug_assert!(cpu < MAX_CPU);
        debug_assert!(idx < MAX_NEST);
        unsafe {
            PER_CPU_NODES
                .get_unchecked_mut(cpu)
                .0
                .get_unchecked_mut(idx)
        }
    }
    /// (0, 0, 1) -> (0, 1, 1) -> (x, 1, 0) -> (x, 0, 1)
    ///       second-cas      wait        store
    #[inline]
    fn second_wait(&self) {
        while self.qval.as_locked().load(Ordering::Relaxed) != 0 {
            core::hint::spin_loop();
        }
        self.qval
            .as_locked_pending()
            .store(QVal::new((0, 0, 1)).locked_pending(), Ordering::Relaxed);
        atomic::fence(Ordering::Acquire);
    }
    #[inline]
    fn third_wait(&self, node: &mut QNode, tail: u16) {
        let mut qval = self.qval.load(Ordering::Relaxed);
        while qval.locked_pending() != 0 {
            core::hint::spin_loop();
            qval = self.qval.load(Ordering::Relaxed);
            continue;
        }
        if qval.tail() == tail {
            // 没有其他排队的节点
            match self
                .qval
                .cas_val(qval, QVal::new((0, 0, 1)), Ordering::Acquire)
            {
                Ok(_) => return, // obtain lock
                Err(v) => {
                    debug_assert_eq!(v.locked_pending(), 0);
                    debug_assert_ne!(v.tail(), tail);
                }
            }
        }
        self.qval.as_locked().store(1, Ordering::Relaxed);
        // 存在排队的节点, 等待它写入的指针到达
        let next = loop {
            if let Some(next) = unsafe { core::ptr::read_volatile(&mut node.next) } {
                break next;
            }
        };
        // 防止 self.qval.locked = 1 和 next.locked = 1 乱序
        atomic::fence(Ordering::Release);
        unsafe {
            core::ptr::write_volatile(&mut (*next.as_ptr()).locked, 1);
        }
    }
    #[inline]
    fn last_wait(&self, old_tail: u16, node: &mut QNode, tail: u16) {
        debug_assert!(node.locked == 0);
        let prev = Self::decode_tail(old_tail);
        prev.next = NonNull::new(node);
        while unsafe { core::ptr::read_volatile(&mut node.locked) } == 0 {
            core::hint::spin_loop();
        }
        // 防止提前观测到未被修改的三元组
        atomic::fence(Ordering::Acquire);
        self.third_wait(node, tail);
    }
}

impl<T: ?Sized + ~const Default, S: MutexSupport> const Default for QSpinLock<T, S> {
    fn default() -> QSpinLock<T, S> {
        QSpinLock::new(Default::default())
    }
}

impl<'a, T: ?Sized, S: MutexSupport> Deref for MutexGuard<'a, T, S> {
    type Target = T;
    #[inline(always)]
    fn deref(&self) -> &T {
        unsafe { &*self.mutex.data.get() }
    }
}

impl<'a, T: ?Sized, S: MutexSupport> DerefMut for MutexGuard<'a, T, S> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<'a, T: ?Sized, S: MutexSupport> Drop for MutexGuard<'a, T, S> {
    /// The dropping of the MutexGuard will release the lock it was created from.
    #[inline(always)]
    fn drop(&mut self) {
        debug_assert!(self.mutex.qval.as_locked().load(Ordering::Relaxed) == 1);
        self.mutex.qval.as_locked().store(0, Ordering::Release);
        S::after_unlock(&mut self.support_guard);
    }
}
