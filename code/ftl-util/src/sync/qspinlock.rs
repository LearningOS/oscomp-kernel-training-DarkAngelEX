use core::{
    cell::SyncUnsafeCell,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    ptr::{addr_of_mut, NonNull},
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
    const fn new((tail, pending, locked): (u16, u8, u8)) -> Self {
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
    #[inline(always)]
    fn pending(self) -> u8 {
        unsafe { self.lpt.pending }
    }
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
    #[inline(always)]
    fn as_pending(&self) -> &AtomicU8 {
        unsafe { core::mem::transmute(&(*self.0.get()).lpt.pending) }
    }
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
    fn fetch_set_pending(&self, order: Ordering) -> QVal {
        let val = self.as_val().fetch_or(QVal::new((0, 1, 0)).val(), order);
        QVal { val }
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
    fn init(&mut self) {
        self.next = None;
        self.locked = 0;
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

struct QMutexGuard<'a, T: ?Sized, S: MutexSupport> {
    mutex: &'a QSpinLock<T, S>,
    support_guard: S::GuardData,
}
// 禁止Mutex跨越await导致死锁或无意义阻塞
impl<'a, T: ?Sized, S: MutexSupport> !Sync for QMutexGuard<'a, T, S> {}
impl<'a, T: ?Sized, S: MutexSupport> !Send for QMutexGuard<'a, T, S> {}

unsafe impl<T: ?Sized + Send, S: MutexSupport> Sync for QSpinLock<T, S> {}
unsafe impl<T: ?Sized + Send, S: MutexSupport> Send for QSpinLock<T, S> {}

impl<T, S: MutexSupport> QSpinLock<T, S> {
    #[inline]
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
    // #[inline(always)]
    pub fn lock(&self) -> impl DerefMut<Target = T> + '_ {
        let support_guard = S::before_lock();
        if let Err(qval) = obtain_lock_fast(&self.qval) {
            obtain_lock(&self.qval, qval);
        }
        QMutexGuard {
            mutex: self,
            support_guard,
        }
    }
}

impl<T: ?Sized + ~const Default, S: MutexSupport> const Default for QSpinLock<T, S> {
    fn default() -> QSpinLock<T, S> {
        QSpinLock::new(Default::default())
    }
}

impl<'a, T: ?Sized, S: MutexSupport> Deref for QMutexGuard<'a, T, S> {
    type Target = T;
    #[inline(always)]
    fn deref(&self) -> &T {
        unsafe { &*self.mutex.data.get() }
    }
}

impl<'a, T: ?Sized, S: MutexSupport> DerefMut for QMutexGuard<'a, T, S> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<'a, T: ?Sized, S: MutexSupport> Drop for QMutexGuard<'a, T, S> {
    /// The dropping of the MutexGuard will release the lock it was created from.
    #[inline(always)]
    fn drop(&mut self) {
        debug_assert!(self.mutex.qval.as_locked().load(Ordering::Relaxed) == 1);
        self.mutex.qval.as_locked().store(0, Ordering::Release);
        S::after_unlock(&mut self.support_guard);
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

/// obtain_lock 内联会导致代码大幅膨胀降低指令缓存命中率, 因此只内联这一部分
#[inline(always)]
fn obtain_lock_fast(this: &QAtomicVal) -> Result<(), QVal> {
    let cur = QVal::new((0, 0, 0));
    let new = QVal::new((0, 0, 1));
    this.cas_val(cur, new, Ordering::Acquire).map(|_| ())
}

fn try_obtain_lock(this: &QAtomicVal) -> bool {
    let qval = this.load(Ordering::Relaxed);
    if qval.val() != QVal::new((0, 0, 0)).val() {
        return false;
    }
    this.cas_val(
        QVal::new((0, 0, 0)),
        QVal::new((0, 0, 1)),
        Ordering::Acquire,
    )
    .is_ok()
}

/// 使用this作为参数来防止不同类型的自旋锁各自生成一份一模一样的垃圾代码, 共享这个函数
fn obtain_lock(this: &QAtomicVal, mut qval: QVal) {
    // this为(0,1,0)的过渡态
    if qval.val() == QVal::new((0, 1, 0)).val() {
        for _ in 0..0x100 {
            qval = this.load(Ordering::Relaxed);
            if qval.val() != QVal::new((0, 1, 0)).val() {
                break;
            }
            core::hint::spin_loop();
        }
    }
    // loop break则进入queue流程
    loop {
        // 如果tail或pending不为0说明不是第一个等待者
        if qval.mask_locked().val() != 0 {
            break;
        }
        // 设置pending位并返回旧的值
        // 不用cas操作! 这种方式可以减少分支数量
        qval = this.fetch_set_pending(Ordering::Acquire);
        // 如果tail或pending不为0说明已经有第一个等待者了
        if qval.mask_locked().val() != 0 {
            // 如果tail存在则释放pending所有权
            if qval.pending() == 0 {
                this.as_pending().store(0, Ordering::Relaxed);
            }
            break;
        }
        // 持有了pending位且tail为0说明是第一个等待着
        // 等待locked被释放
        if qval.locked() != 0 {
            while this.as_locked().load(Ordering::Relaxed) != 0 {
                core::hint::spin_loop();
            }
        }
        this.as_locked_pending()
            .store(QVal::new((0, 0, 1)).locked_pending(), Ordering::Relaxed);
        atomic::fence(Ordering::Acquire); // 锁内存屏障
        return;
    }
    // 慢速路径开始, 进入队列
    // 不需要获取pending位, 而是等待locked为true成为第二个等待着, 再等待(x,0,0)状态获取锁
    let cpuid = ftl_local().cpuid();
    let cpu_nodes = current_mcs(cpuid);
    let idx = cpu_nodes.0[0].count as usize;
    debug_assert!(idx < MAX_NEST);
    let count_ptr = addr_of_mut!(cpu_nodes.0[0].count);
    let tail = encode_tail(cpuid, idx);
    unsafe { *count_ptr += 1 }
    atomic::compiler_fence(Ordering::Release); // 保证递增操作在init之前
    let node = &mut cpu_nodes.0[idx];
    node.init();
    // 判断锁是否已经被释放了
    if try_obtain_lock(this) {
        unsafe { *count_ptr -= 1 }
        return;
    }
    // 防止node.init乱序到swap后面
    atomic::fence(Ordering::Release);
    // 使用atomic swap交换tail, riscv的16位原子交换是cas实现的
    let old = this.as_tail().swap(tail, Ordering::Relaxed);
    let mut next = None;
    // 如果old存在则此节点已经插入了队列, 此时要设置它的next指针
    if old != 0 {
        let prev = decode_tail(old);
        // 写入next
        unsafe { core::ptr::write_volatile(&mut prev.next, NonNull::new(node)) }
        // 等待自身成为第一个节点
        while unsafe { core::ptr::read_volatile(&node.locked) == 0 } {
            core::hint::spin_loop();
        }
        // prefetch 内存
        next = unsafe { core::ptr::read_volatile(&node.next) };
        if let Some(next) = next {
            // 预读next所在的缓存块
            unsafe { core::ptr::read_volatile(&(*next.as_ptr()).locked) };
        }
    }
    qval = this.load(Ordering::Relaxed);
    // 等待状态(x, 0, 0)
    while qval.locked_pending() != 0 {
        core::hint::spin_loop();
        qval = this.load(Ordering::Relaxed);
    }
    if qval.tail() == tail {
        if this
            .cas_val(qval, QVal::new((0, 0, 1)), Ordering::Relaxed)
            .is_ok()
        {
            unsafe { *count_ptr -= 1 }
            return;
        }
    }
    this.as_locked().store(1, Ordering::Relaxed);
    if next.is_none() {
        next = unsafe { core::ptr::read_volatile(&node.next) };
        while next.is_none() {
            core::hint::spin_loop();
            next = unsafe { core::ptr::read_volatile(&node.next) };
        }
    }
    atomic::fence(Ordering::AcqRel);
    unsafe { core::ptr::write_volatile(&mut next.unwrap().as_mut().locked, 1) }
    unsafe { *count_ptr -= 1 }
}
