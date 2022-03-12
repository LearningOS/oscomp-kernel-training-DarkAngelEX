/// 逻辑参考自 https://coolshell.cn/articles/8239.html
///
/// 本队列的优势除了无锁, 更重要的是使用时不需要关中断。
use core::{
    mem::MaybeUninit,
    ptr::NonNull,
    sync::atomic::{AtomicUsize, Ordering},
};

use alloc::boxed::Box;

use super::marked_ptr::{AtomicMarkedPtr, MarkedPtr, PtrID};

// 无锁单向链表
pub struct LockFreeQueue<T> {
    head: AtomicMarkedPtr<LockFreeNode<T>>,
    tail: AtomicMarkedPtr<LockFreeNode<T>>,
    unique: AtomicUsize,
}

unsafe impl<T: Send> Send for LockFreeQueue<T> {}
unsafe impl<T> Sync for LockFreeQueue<T> {}

///
pub struct ThreadLocalLinkedList<T> {
    head: MarkedPtr<ThreadLocalNode<T>>,
}

struct LockFreeNode<T> {
    next: AtomicMarkedPtr<Self>,
    value: MaybeUninit<T>,
}

struct ThreadLocalNode<T> {
    next: MarkedPtr<Self>,
    value: MaybeUninit<T>,
}

impl<T> Drop for LockFreeQueue<T> {
    fn drop(&mut self) {
        let mut head = self.head.get();
        let tail = self.head.get();
        if head.get_ptr().is_none() {
            debug_check!(tail.get_ptr().is_none());
            return;
        }
        unsafe {
            while head != tail {
                let this = head.get_ptr().unwrap().as_mut();
                head = this.next.get();
                drop(head.get_ptr().unwrap().as_mut().value.assume_init_read());
                drop(Box::from_raw(this));
            }
            // skip drop dummy
            drop(Box::from_raw(head.get_ptr().unwrap().as_ptr()));
        }
    }
}

impl<T> LockFreeQueue<T> {
    pub const fn new() -> Self {
        Self {
            head: AtomicMarkedPtr::null(),
            tail: AtomicMarkedPtr::null(),
            unique: AtomicUsize::new(0),
        }
    }
    pub fn init(&mut self) {
        let mut dummy = unsafe { Box::<LockFreeNode<T>>::new_uninit().assume_init() };
        dummy.next = AtomicMarkedPtr::null();
        let ptr: Option<NonNull<_>> = Some(Box::leak(dummy).into());
        self.head = AtomicMarkedPtr::new(MarkedPtr::new(PtrID::zero(), ptr));
        self.tail = AtomicMarkedPtr::new(MarkedPtr::new(PtrID::zero(), ptr));
    }
    pub unsafe fn init_uncheck(&self) {
        let ptr = self as *const _ as *mut Self;
        (*ptr).init();
    }
    /// 只能同时有一个线程进行这个操作! push pop 不需要被阻塞
    ///
    /// 如果多个线程同时调用这个函数会导致 tail 指向错误的链表
    pub fn take_all(&self) -> Result<ThreadLocalLinkedList<T>, ()> {
        stack_trace!();

        debug_check!(self
            .unique
            .compare_exchange(0, 1, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok());

        // 先让 tail 指向 new_dummy, 再让 head 指向 new_dummy
        let new_dummy = LockFreeNode::<T> {
            next: AtomicMarkedPtr::null(),
            value: MaybeUninit::uninit(),
        };
        let new_dummy: Option<NonNull<_>> = Some(Box::leak(Box::new(new_dummy)).into());

        let mut tail = self.tail.get();
        loop {
            let next_v = tail.get_mut().ok_or(())?.next.get();
            let cur_tail = self.tail.get();
            if tail != cur_tail {
                tail = cur_tail;
                continue;
            }
            if next_v.get_mut().is_some() {
                // tail 不是真正的队尾
                let new_tail = MarkedPtr::new(tail.id(), next_v.get_ptr());
                tail = match self.tail.compare_exchange(tail, new_tail) {
                    Ok(_) => new_tail,
                    Err(cur_tail) => cur_tail,
                };
                core::hint::spin_loop();
                continue;
            }

            let new_tail = MarkedPtr::new(tail.id(), new_dummy);
            match self.tail.compare_exchange(tail, new_tail) {
                Ok(_) => break,
                Err(cur_tail) => {
                    tail = cur_tail;
                    core::hint::spin_loop();
                    continue;
                }
            }
        }
        let mut head = self.head.get();
        loop {
            // 禁止 take 函数发生时被另一个进程关闭
            head.get_ptr().unwrap();
            let new_head = MarkedPtr::new(head.id(), new_dummy);
            match self.head.compare_exchange(head, new_head) {
                Ok(_) => break,
                Err(cur_head) => {
                    head = cur_head;
                    core::hint::spin_loop();
                    continue;
                }
            }
        }

        debug_check!(self
            .unique
            .compare_exchange(1, 0, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok());

        let next = head.get_mut().unwrap().next.get().cast();
        unsafe { Box::from_raw(head.get_ptr().unwrap().as_ptr()) };
        Ok(ThreadLocalLinkedList { head: next })
    }
    pub fn close(&self) -> Result<ThreadLocalLinkedList<T>, ()> {
        stack_trace!();
        // 顺序: 先禁止入队(tail) 再禁止出队(head)
        let mut tail = self.tail.get();
        loop {
            tail.get_ptr().ok_or(())?;
            match self.tail.compare_exchange(tail, tail.into_null()) {
                Ok(_) => break,
                Err(cur_tail) => {
                    tail = cur_tail;
                    core::hint::spin_loop();
                }
            }
        }
        // 如果tail关闭成功了, head一定有效, 其他尝试关闭的线程都将失败.
        let mut head = self.head.get();
        loop {
            head.get_ptr().unwrap();
            match self.head.compare_exchange(head, head.into_null()) {
                Ok(_) => break,
                Err(cur_head) => {
                    head = cur_head;
                    core::hint::spin_loop();
                }
            }
        }
        let next = head.get_mut().unwrap().next.get().cast();
        unsafe { Box::from_raw(head.get_ptr().unwrap().as_ptr()) };
        Ok(ThreadLocalLinkedList { head: next })
    }
    pub fn push(&self, value: T) -> Result<(), ()> {
        stack_trace!();
        let node = LockFreeNode::<T> {
            next: AtomicMarkedPtr::null(),
            value: MaybeUninit::new(value),
        };
        let new_node: Option<NonNull<_>> = Some(Box::leak(Box::new(node)).into());
        // tail 定义为一定在 head 之后且距离队列尾很近的标记.
        let mut tail = self.tail.get();
        loop {
            let next = &tail.get_mut().ok_or(())?.next;
            // tail可能已经被释放, 这个指针的值可能是无效值
            let next_ptr = next.get();
            let next_v = next_ptr.get_ptr();
            // tail 有效则保证 next_v 有效
            let cur_tail = self.tail.get();
            if tail != cur_tail {
                tail = cur_tail;
                core::hint::spin_loop();
                continue;
            }
            if next_v.is_some() {
                // tail 不是真正的队尾
                let new_tail = MarkedPtr::new(tail.id(), next_v);
                tail = match self.tail.compare_exchange(tail, new_tail) {
                    Ok(_) => new_tail,
                    Err(cur_tail) => cur_tail,
                };
                core::hint::spin_loop();
                continue;
            }
            let new_tail = MarkedPtr::new(next_ptr.id(), new_node);
            match next.compare_exchange(next_ptr, new_tail) {
                Ok(_) => {
                    let new_tail = MarkedPtr::new(tail.id(), new_node);
                    let _ = self.tail.compare_exchange(tail, new_tail);
                    return Ok(());
                }
                Err(cur_tail) => {
                    tail = cur_tail;
                    core::hint::spin_loop();
                    continue;
                }
            }
        }
    }
    pub fn pop(&self) -> Result<Option<T>, ()> {
        stack_trace!();
        // self.head 必然是合法地址, 即使被释放了也可以取到无效数据而不会异常 放在 loop 外减少一次 fetch
        let mut head = self.head.get();
        loop {
            // 如果 head 为空指针则表示队列已经被关闭.
            let next = &head.get_mut().ok_or(())?.next;
            // head 可能在这里被释放, 下面的 next.get() 取到了无效值
            let next_ptr = next.get();
            // 如果观测到 next_ptr 是无效数据, 由于 self.head.cas 包含 Acquire 内存序 下面的内存释放无法排序到 self.head 修改之前
            // 一定导致接下来的 self.head.get() 观测到修改, 重新开始循环.
            let cur_head = self.head.get();
            // 保存 cur_head 避免额外的 fetch
            if head != cur_head {
                head = cur_head;
                core::hint::spin_loop();
                continue;
            }
            let next_v = next_ptr.get_ptr();
            // 为了take()函数并行化, 这里不判断 head == tail
            if next_v.is_none() {
                return Ok(None);
            }
            // 避免tail落后head
            let tail = self.tail.get();
            // 防止关闭的队列被重新打开
            tail.get_ptr().ok_or(())?;
            if head == tail {
                let new_tail = MarkedPtr::new(tail.id(), next_v);
                let _ = self.tail.compare_exchange(tail, new_tail);
                core::hint::spin_loop();
                continue;
            }
            let new_head = MarkedPtr::new(head.id(), next_v);
            // 避免 value 被另一个线程 pop 后释放, 提前读数据, 这个数据可能是无效值 next_v 已经被判断.
            let value = unsafe { core::ptr::read(&next_v.unwrap().as_mut().value) };
            // 使用 SeqCst 保证 value 取值在 cas 之前, 释放head在 cas 之后
            match self.head.compare_exchange(head, new_head) {
                Ok(_) => {
                    // 如果 CAS 成功则拥有了 head 与 value 的所有权. head 已经被非空判断.
                    unsafe { Box::from_raw(head.get_ptr().unwrap().as_ptr()) };
                    // next become new dummy
                    let value = unsafe { value.assume_init_read() };
                    return Ok(Some(value));
                }
                Err(cur_head) => {
                    head = cur_head;
                    core::hint::spin_loop();
                    continue;
                }
            }
        }
    }
}

pub mod test {
    use alloc::{
        collections::{LinkedList, VecDeque},
        vec::Vec,
    };

    use crate::{sync::mutex::SpinLock, timer, tools};

    use super::LockFreeQueue;

    fn test_push_pop_impl(
        hart: usize,
        producer: usize,
        consumer: usize,
        total: usize,
        push: impl Fn(usize),
        pop: impl Fn() -> Option<usize>,
        off: usize,
    ) {
        use crate::hart::cpu;

        assert!(producer + consumer <= cpu::count());
        tools::wait_all_hart();
        let begin = timer::get_time_ticks();
        let n = if hart < producer {
            let n = if hart != producer - 1 {
                total / producer
            } else {
                total - total / producer * (producer - 1)
            };
            let begin = total / producer * hart;
            for i in begin..begin + n {
                push(i);
            }
            n
        } else if hart >= producer && hart < producer + consumer {
            let n = if hart != producer + consumer - 1 {
                total / consumer
            } else {
                total - total / consumer * (consumer - 1)
            };
            unsafe {
                SET_TABLE[hart].clear();
                for _i in 0..n {
                    let mut retry = 0;
                    loop {
                        retry += 1;
                        if retry > 10000 {
                            panic!();
                        }
                        if let Some(v) = pop() {
                            SET_TABLE[hart].push(v);
                            break;
                        }
                    }
                }
            }
            n
        } else {
            0
        };
        let end = timer::get_time_ticks();
        for i in 0..cpu::count() {
            tools::wait_all_hart();
            if i == hart {
                println!(
                    "{}hart {} n:{} time: {}ms",
                    tools::n_space(off),
                    hart,
                    n,
                    (end - begin).into_millisecond()
                );
            }
        }
    }

    pub fn check() {
        use alloc::collections::BTreeSet;
        unsafe {
            assert_eq!(TEST_QUEUE_0.pop().unwrap(), None);
            let mut set = BTreeSet::new();
            for v in SET_TABLE.iter_mut() {
                for i in &*v {
                    assert!(set.insert(*i));
                }
                v.clear();
            }
            let mut cnt = 0;
            for &i in set.iter() {
                // print!("{} ", i);
                assert_eq!(i, cnt);
                cnt += 1;
            }
        }
    }

    fn test_impl(
        hart: usize,
        producer: usize,
        consumer: usize,
        total: usize,
        push: impl Fn(usize),
        pop: impl Fn() -> Option<usize>,
        off: usize,
    ) {
        if hart == 0 {
            println!(
                "{}lock free queue test producer: {} consumer: {} total: {}",
                tools::n_space(off),
                producer,
                consumer,
                total
            );
        }
        tools::wait_all_hart();
        test_push_pop_impl(hart, producer, consumer, total, push, pop, off + 4);
        tools::wait_all_hart();
        if hart == 0 {
            println!("{}check begin", tools::n_space(off));
            check();
            println!("{}check pass", tools::n_space(off));
        }
    }

    static TEST_QUEUE_0: LockFreeQueue<usize> = LockFreeQueue::new();
    static TEST_QUEUE_1: SpinLock<LinkedList<usize>> = SpinLock::new(LinkedList::new());
    static TEST_QUEUE_2: SpinLock<core::lazy::Lazy<VecDeque<usize>>> =
        SpinLock::new(core::lazy::Lazy::new(|| VecDeque::new()));
    static mut SET_TABLE: [Vec<usize>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];

    pub fn multi_thread_test(hart: usize, off: usize) {
        stack_trace!();
        if hart == 0 {
            println!(
                "{}lock lock_free queue multi_thread_test begin",
                tools::n_space(off)
            );
            unsafe { TEST_QUEUE_0.init_uncheck() };
        }
        tools::wait_all_hart();
        let push = |a| TEST_QUEUE_0.push(a).unwrap();
        let pop = || TEST_QUEUE_0.pop().unwrap();
        test_impl(hart, 1, 3, 10000, push, pop, off + 4);
        test_impl(hart, 2, 2, 10000, push, pop, off + 4);
        test_impl(hart, 3, 1, 10000, push, pop, off + 4);
        tools::wait_all_hart();
        if hart == 0 {
            println!(
                "{}locked linked list multi_thread_test begin",
                tools::n_space(off)
            );
        }
        let push = |a| TEST_QUEUE_1.lock(place!()).push_back(a);
        let pop = || TEST_QUEUE_1.lock(place!()).pop_front();
        test_impl(hart, 1, 3, 10000, push, pop, off + 4);
        test_impl(hart, 2, 2, 10000, push, pop, off + 4);
        test_impl(hart, 3, 1, 10000, push, pop, off + 4);
        if hart == 0 {
            println!(
                "{}locked VecDeque multi_thread_test begin",
                tools::n_space(off)
            );
        }
        let push = |a| {
            let v = &**TEST_QUEUE_2.lock(place!());
            unsafe { &mut *(v as *const _ as *mut VecDeque<usize>) }.push_back(a)
        };
        let pop = || {
            let v = &**TEST_QUEUE_2.lock(place!());
            unsafe { &mut *(v as *const _ as *mut VecDeque<usize>) }.pop_front()
        };
        test_impl(hart, 1, 3, 10000, push, pop, off + 4);
        test_impl(hart, 2, 2, 10000, push, pop, off + 4);
        test_impl(hart, 3, 1, 10000, push, pop, off + 4);
        tools::wait_all_hart();
        if hart == 0 {
            println!(
                "{}lock free queue multi_thread_test pass",
                tools::n_space(off)
            );
        }
    }
}
