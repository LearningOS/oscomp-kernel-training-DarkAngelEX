/// 逻辑参考自 https://coolshell.cn/articles/8239.html
///
/// 无锁队列的优势除了无锁, 更重要的是使用时不需要关中断。
///
/// 压力测试中约1亿次操作出错一次，原因未知
use core::{mem::MaybeUninit, ptr::NonNull, sync::atomic::AtomicUsize};

use alloc::boxed::Box;

use crate::tools::container::thread_local_linked_list::ThreadLocalLinkedList;

use super::marked_ptr::{AtomicMarkedPtr, MarkedPtr, PtrID};

const QUEUE_DEBUG: bool = true;
// 无锁单向链表
pub struct LockfreeQueue<T> {
    head: AtomicMarkedPtr<LockfreeNode<T>>,
    tail: AtomicMarkedPtr<LockfreeNode<T>>,
    unique: AtomicUsize,
}

unsafe impl<T: Send> Send for LockfreeQueue<T> {}
unsafe impl<T> Sync for LockfreeQueue<T> {}

struct LockfreeNode<T> {
    // _safety: usize,
    next: AtomicMarkedPtr<Self>,
    value: MaybeUninit<T>,
}

impl<T> LockfreeNode<T> {
    fn new(value: T) -> Self {
        LockfreeNode {
            // _safety: 0,
            next: AtomicMarkedPtr::null(),
            value: MaybeUninit::new(value),
        }
    }
    fn dummy() -> Self {
        Self {
            // _safety: 0,
            next: AtomicMarkedPtr::null(),
            value: MaybeUninit::uninit(),
        }
    }
}

impl<T> Drop for LockfreeQueue<T> {
    fn drop(&mut self) {
        let mut head = self.head.load();
        let tail = self.head.load();
        if head.get_ptr().is_none() {
            debug_check!(tail.get_ptr().is_none());
            return;
        }
        unsafe {
            while head != tail {
                let this = head.get_ptr().unwrap().as_mut();
                head = this.next.load();
                drop(head.get_ptr().unwrap().as_mut().value.assume_init_read());
                drop(Box::from_raw(this));
            }
            // skip drop dummy
            drop(Box::from_raw(head.get_ptr().unwrap().as_ptr()));
        }
    }
}

impl<T> LockfreeQueue<T> {
    pub const fn new() -> Self {
        Self {
            head: AtomicMarkedPtr::null(),
            tail: AtomicMarkedPtr::null(),
            unique: AtomicUsize::new(0),
        }
    }
    pub fn init(&mut self) {
        let mut dummy = unsafe {
            let mut p = Box::<LockfreeNode<T>>::new_uninit();
            if QUEUE_DEBUG {
                (*p.as_mut_ptr()).value.write(core::mem::zeroed());
            }
            p.assume_init()
        };
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
        // 先让 tail 指向 new_dummy, 再让 head 指向 new_dummy
        let new_dummy = LockfreeNode::dummy();
        let new_dummy: &mut LockfreeNode<T> = Box::leak(Box::new(new_dummy));
        let mut tail = self.tail.load();
        loop {
            new_dummy.next.set_id_null(tail.id());
            let new_tail = MarkedPtr::new(tail.id(), Some(new_dummy.into()));
            match self.tail.compare_exchange(tail, new_tail) {
                Ok(cur_tail) => {
                    tail = cur_tail;
                    break;
                }
                Err(cur_tail) => {
                    tail = cur_tail;
                    core::hint::spin_loop();
                    continue;
                }
            }
        }
        let mut head = self.head.load();
        // print_n(2);
        loop {
            // 禁止 take 函数发生时被另一个进程关闭
            head.get_ptr().unwrap();
            let new_head = MarkedPtr::new(head.id(), Some(new_dummy.into()));
            match self.head.compare_exchange(head, new_head) {
                Ok(_) => break,
                Err(cur_head) => {
                    head = cur_head;
                    core::hint::spin_loop();
                    continue;
                }
            }
        }
        // change marker to forbid push or pop.
        let head_next = &head.get_mut().unwrap().next;
        let mut next_v = head_next.load();
        loop {
            let null = MarkedPtr::null(next_v.id());
            match head_next.compare_exchange(next_v, null) {
                Ok(_) => break,
                Err(x) => next_v = x,
            }
        }
        let tail_next = &tail.get_mut().unwrap().next;
        let mut tail_next_v = tail_next.load();
        loop {
            match tail_next.compare_exchange(tail_next_v, tail_next_v) {
                Ok(_) => break,
                Err(x) => tail_next_v = x,
            }
        }
        let list = ThreadLocalLinkedList::ptr_new(next_v.cast());
        // let x2: usize = unsafe { core::mem::transmute(MarkedPtr::<usize>::null(next_v.id())) };
        // let next = head.get_mut().unwrap().next.load();
        // let x3: usize = unsafe { core::mem::transmute(next) };
        // println!("{} {:#x} {:#x} {:#x} ", xlen, x1, x2, x3);
        // assert_eq!(x2 + (1 << 39), x3);
        unsafe { Box::from_raw(head.get_ptr().unwrap().as_ptr()) };
        Ok(list)
    }
    pub fn close(&self) -> Result<ThreadLocalLinkedList<T>, ()> {
        stack_trace!();
        // 顺序: 先禁止入队(tail) 再禁止出队(head)
        let mut tail = self.tail.load();
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
        let mut head = self.head.load();
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
        let next = head.get_mut().unwrap().next.load().cast();
        unsafe { Box::from_raw(head.get_ptr().unwrap().as_ptr()) };
        Ok(ThreadLocalLinkedList::ptr_new(next))
    }
    pub fn push(&self, value: T) -> Result<(), ()> {
        fn block() {
            for i in 0..1000 {
                core::hint::black_box(i);
            }
        }
        stack_trace!();
        let node = LockfreeNode::new(value);
        let mut new_node: NonNull<_> = Box::leak(Box::new(node)).into();
        // tail 定义为一定在 head 之后且距离队列尾很近的标记.
        let mut tail = self.tail.load();
        loop {
            // 只是tail的偏移量而已
            let next = &tail.get_mut().ok_or(())?.next;
            // tail可能已经被释放, 这个指针的值可能是无效值
            let next_ptr = next.load();
            let next_v = next_ptr.get_ptr();
            // tail 有效则保证 next_v 有效
            let cur_tail = self.tail.load();
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
            // self.debug_block(300);
            // 防止 ABA 错误: new_node.next.id初始化为0, 缺少版本号
            unsafe {
                new_node.as_mut().next.set_id_null(tail.id());
            }

            // debug release check
            let vp = unsafe {
                if QUEUE_DEBUG {
                    let p = &tail.get_mut().unwrap().value as *const _ as *const u32;
                    *p
                } else {
                    0
                }
            };
            let new_tail = MarkedPtr::new(next_ptr.id(), Some(new_node));
            // block();
            // next 延迟修改会导致 take_all 无法正确获得最新 next
            match next.compare_exchange(next_ptr, new_tail) {
                Ok(_) => {
                    // tail已经被释放了!!!
                    if QUEUE_DEBUG && vp == 0xf0f0f0f0 {
                        unsafe {
                            let next_ptr: usize = core::mem::transmute(next_ptr);
                            let new_tail: usize = core::mem::transmute(new_tail);
                            panic!("{:#x} {:#x}", next_ptr, new_tail);
                        }
                    }
                    let new_tail = MarkedPtr::new(tail.id(), Some(new_node));
                    let _ = self.tail.compare_exchange(tail, new_tail);
                    return Ok(());
                }
                Err(_) => {
                    tail = self.tail.load();
                    core::hint::spin_loop();
                    continue;
                }
            }
        }
    }
    pub fn pop(&self) -> Result<Option<T>, ()> {
        fn block() {
            for i in 0..1000 {
                core::hint::black_box(i);
            }
        }
        stack_trace!();
        // self.head 必然是合法地址, 即使被释放了也可以取到无效数据而不会异常 放在 loop 外减少一次 fetch
        let mut head = self.head.load();
        loop {
            // 如果 head 为空指针则表示队列已经被关闭.
            let next = &head.get_mut().ok_or(())?.next;
            let tail = self.tail.load();
            // 防止关闭的队列被重新打开
            tail.get_ptr().ok_or(())?;
            // head 可能在这里被释放, 下面的 next.get() 取到了无效值
            let next_ptr = next.load();
            // 如果观测到 next_ptr 是无效数据, 由于 self.head.cas 包含 Acquire 内存序 下面的内存释放无法排序到 self.head 修改之前
            // 一定导致接下来的 self.head.get() 观测到修改, 重新开始循环.
            let cur_head = self.head.load();
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
            if head.get_ptr() == tail.get_ptr() {
                const NEVER_TAKE_ALL: bool = false;
                if NEVER_TAKE_ALL && QUEUE_DEBUG && head.id() != tail.id() {
                    println!("id:{} {}", head.id().num(), tail.id().num());
                    panic!();
                }
                // print!("*");
                let new_tail = MarkedPtr::new(tail.id(), next_v);
                let _ = self.tail.compare_exchange(tail, new_tail);
                core::hint::spin_loop();
                head = self.head.load();
                continue;
            }
            let new_head = MarkedPtr::new(head.id(), next_v);
            // 避免 value 被另一个线程 pop 后释放, 提前读数据, 这个数据可能是无效值 next_v 已经被判断.
            let value = unsafe { core::ptr::read(&next_v.unwrap().as_mut().value) };
            // 使用 SeqCst 保证 value 取值在 cas 之前, 释放head在 cas 之后
            // block();
            match self.head.compare_exchange(head, new_head) {
                Ok(_) => {
                    // 如果 CAS 成功则拥有了 head 与 value 的所有权. head 已经被非空判断.
                    unsafe {
                        let old_head = head.get_ptr().unwrap().as_ptr();
                        // (*old_head).next.confusion();
                        // 释放内存
                        Box::from_raw(old_head);
                        // next become new dummy
                        let value = value.assume_init_read();
                        return Ok(Some(value));
                    }
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

    use core::sync::atomic::{AtomicUsize, Ordering};

    use alloc::{
        collections::{LinkedList, VecDeque},
        vec::Vec,
    };

    use crate::{
        sync::mutex::SpinNoIrqLock,
        timer,
        tools::{
            self,
            container::{
                lockfree::stack::LockfreeStack, thread_local_linked_list::ThreadLocalLinkedList,
            },
        },
    };

    use super::LockfreeQueue;

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
                for i in 0..n {
                    let mut retry = 0;
                    loop {
                        retry += 1;
                        if retry > 10000000 {
                            panic!("{}", i);
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
            for (cnt, &i) in set.iter().enumerate() {
                // print!("{} ", i);
                assert_eq!(i, cnt);
            }
        }
    }

    fn group_test_impl(
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
                "{}group test producer: {} consumer: {} total: {}",
                tools::n_space(off),
                producer,
                consumer,
                total
            );
        }
        stack_trace!();
        tools::wait_all_hart();
        let t0 = timer::get_time_ticks();
        tools::wait_all_hart();
        test_push_pop_impl(hart, producer, consumer, total, push, pop, off + 4);
        tools::wait_all_hart();
        let t1 = timer::get_time_ticks();
        tools::wait_all_hart();
        if hart == 0 {
            let ms = (t1 - t0).into_millisecond();
            println!("{}time: {}ms", tools::n_space(off), ms);
            if false {
                println!("{}check begin", tools::n_space(off));
                check();
                println!("{}check pass", tools::n_space(off));
            } else {
                println!("{}skip check", tools::n_space(off));
            }
        }
    }
    static COUNT_PUSH: AtomicUsize = AtomicUsize::new(0);
    static COUNT_POP: AtomicUsize = AtomicUsize::new(0);
    fn gather_test_impl(
        hart: usize,
        batch: usize,
        total: usize,
        push: impl Fn(usize),
        pop: impl Fn() -> Option<usize>,
        off: usize,
    ) {
        stack_trace!();
        if hart == 0 {
            COUNT_PUSH.store(0, Ordering::Relaxed);
            COUNT_POP.store(0, Ordering::Relaxed);
            println!(
                "{}gather test total: {} batch: {}",
                tools::n_space(off),
                total,
                batch
            );
        }
        tools::wait_all_hart();
        unsafe { SET_TABLE[hart].clear() };
        let t0 = timer::get_time_ticks();
        tools::wait_all_hart();
        loop {
            let begin = COUNT_PUSH.fetch_add(batch, Ordering::Relaxed);
            if begin >= total {
                break;
            }
            let end = (begin + batch).min(total);
            for i in begin..end {
                push(i);
            }
            for i in begin..end {
                let mut retry = 0;
                loop {
                    retry += 1;
                    if retry > 10000 {
                        panic!("{}", i);
                    }
                    if let Some(_v) = pop() {
                        // unsafe { SET_TABLE[hart].push(v) };
                        break;
                    }
                }
            }
        }
        tools::wait_all_hart();
        let t1 = timer::get_time_ticks();
        tools::wait_all_hart();
        if hart == 0 {
            let ms = (t1 - t0).into_millisecond();
            println!("{}time: {}ms", tools::n_space(off), ms);
            assert_eq!(TEST_QUEUE_0.pop().unwrap(), None);
            if false {
                println!("{}check begin", tools::n_space(off));
                check();
                println!("{}check pass", tools::n_space(off));
            } else {
                println!("{}skip check", tools::n_space(off));
            }
        }
    }
    fn take_all_test(
        hart: usize,
        total: usize,
        push: impl Fn(usize),
        pop: impl Fn() -> Option<usize>,
        take_all: impl Fn() -> ThreadLocalLinkedList<usize>,
        off: usize,
    ) {
        stack_trace!();
        if hart == 0 {
            COUNT_PUSH.store(0, Ordering::Relaxed);
            COUNT_POP.store(0, Ordering::Relaxed);
            println!("{}take_all_test total: {}", tools::n_space(off), total,);
        }
        tools::wait_all_hart();
        unsafe { SET_TABLE[hart].clear() };
        let t0 = timer::get_time_ticks();
        tools::wait_all_hart();

        match hart {
            0 => {
                const IN_ORDER: bool = false;
                let mut xv = 0;
                loop {
                    if COUNT_PUSH.load(Ordering::Relaxed) >= total {
                        break;
                    }
                    let mut list = take_all();
                    let mut cnt = 0;
                    while let Some(v) = list.pop() {
                        if IN_ORDER {
                            assert_eq!(xv, v);
                            xv += 1;
                        }
                        unsafe { SET_TABLE[hart].push(v) };
                        cnt += 1;
                    }
                    COUNT_POP.fetch_add(cnt, Ordering::Relaxed);
                }
            }
            1 => loop {
                if COUNT_PUSH.load(Ordering::Relaxed) >= total {
                    break;
                }
                if let Some(v) = pop() {
                    unsafe { SET_TABLE[hart].push(v) };
                    COUNT_POP.fetch_add(1, Ordering::Relaxed);
                }
            },
            2 => {
                const BATCH: usize = 1000;
                loop {
                    let begin = COUNT_PUSH.fetch_add(BATCH, Ordering::Relaxed);
                    if begin >= total {
                        break;
                    }
                    let end = (begin + BATCH).min(total);
                    for i in begin..end {
                        push(i);
                    }
                }
            }
            _ => (),
        }
        tools::wait_all_hart();
        if hart == 0 {
            let xpop = COUNT_POP.load(Ordering::SeqCst);
            for i in xpop..total {
                let v = match pop() {
                    Some(v) => v,
                    None => panic!("start:{} have: {} total: {}", xpop, i, total),
                };
                unsafe { SET_TABLE[hart].push(v) };
            }
            assert!(pop().is_none());
        }
        let t1 = timer::get_time_ticks();
        tools::wait_all_hart();
        if hart == 0 {
            let ms = (t1 - t0).into_millisecond();
            println!("{}time: {}ms", tools::n_space(off), ms);
            assert_eq!(TEST_QUEUE_0.pop().unwrap(), None);
            if true {
                println!("{}check begin", tools::n_space(off));
                check();
                println!("{}check pass", tools::n_space(off));
            } else {
                println!("{}skip check", tools::n_space(off));
            }
        }
    }

    type Mutex<T> = SpinNoIrqLock<T>;
    // type Mutex<T> = SpinLock<T>;
    static TEST_QUEUE_0: LockfreeQueue<usize> = LockfreeQueue::new();
    static TEST_QUEUE_1: Mutex<LinkedList<usize>> = Mutex::new(LinkedList::new());
    static TEST_QUEUE_2: Mutex<core::lazy::Lazy<VecDeque<usize>>> =
        Mutex::new(core::lazy::Lazy::new(VecDeque::new));
    static TEST_QUEUE_3: LockfreeStack<usize> = LockfreeStack::new();
    static mut SET_TABLE: [Vec<usize>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];

    pub fn multi_thread_performance_test(hart: usize, off: usize) {
        stack_trace!();
        if hart == 0 {
            println!("{}lock_free queue test begin", tools::n_space(off));
            unsafe { TEST_QUEUE_0.init_uncheck() };
        }
        const TOTAL: usize = 100000;
        tools::wait_all_hart();
        let push = |a| {
            TEST_QUEUE_0.push(a).unwrap();
        };
        let pop = || TEST_QUEUE_0.pop().unwrap();
        group_test_impl(hart, 1, 1, TOTAL, push, pop, off + 4);
        group_test_impl(hart, 1, 3, TOTAL, push, pop, off + 4);
        group_test_impl(hart, 2, 2, TOTAL, push, pop, off + 4);
        group_test_impl(hart, 3, 1, TOTAL, push, pop, off + 4);
        tools::wait_all_hart();
        if hart == 0 {
            println!("{}locked linked list test begin", tools::n_space(off));
        }
        let push = |a| TEST_QUEUE_1.lock(place!()).push_back(a);
        let pop = || TEST_QUEUE_1.lock(place!()).pop_front();
        group_test_impl(hart, 1, 1, TOTAL, push, pop, off + 4);
        group_test_impl(hart, 1, 3, TOTAL, push, pop, off + 4);
        group_test_impl(hart, 2, 2, TOTAL, push, pop, off + 4);
        group_test_impl(hart, 3, 1, TOTAL, push, pop, off + 4);
        if hart == 0 {
            println!("{}locked VecDeque test begin", tools::n_space(off));
        }
        let push = |a| {
            let v = &**TEST_QUEUE_2.lock(place!());
            #[allow(clippy::cast_ref_to_mut)]
            unsafe { &mut *(v as *const _ as *mut VecDeque<usize>) }.push_back(a)
        };
        let pop = || {
            let v = &**TEST_QUEUE_2.lock(place!());
            #[allow(clippy::cast_ref_to_mut)]
            unsafe { &mut *(v as *const _ as *mut VecDeque<usize>) }.pop_front()
        };
        group_test_impl(hart, 1, 1, TOTAL, push, pop, off + 4);
        group_test_impl(hart, 1, 3, TOTAL, push, pop, off + 4);
        group_test_impl(hart, 2, 2, TOTAL, push, pop, off + 4);
        group_test_impl(hart, 3, 1, TOTAL, push, pop, off + 4);
        tools::wait_all_hart();
        if hart == 0 {
            println!("{}lock_free stack test begin", tools::n_space(off));
        }
        let push = |a| TEST_QUEUE_3.push(a).unwrap();
        let pop = || TEST_QUEUE_3.pop().unwrap();
        group_test_impl(hart, 1, 1, TOTAL, push, pop, off + 4);
        group_test_impl(hart, 1, 3, TOTAL, push, pop, off + 4);
        group_test_impl(hart, 2, 2, TOTAL, push, pop, off + 4);
        group_test_impl(hart, 3, 1, TOTAL, push, pop, off + 4);
        tools::wait_all_hart();
        if hart == 0 {
            println!(
                "{}lock free stack multi_thread_performance_test pass",
                tools::n_space(off)
            );
        }
    }
    pub fn multi_thread_stress_test(hart: usize, off: usize) {
        stack_trace!();
        if hart == 0 {
            println!(
                "{}lock_free queue multi_thread_stress_test begin",
                tools::n_space(off)
            );
            unsafe { TEST_QUEUE_0.init_uncheck() };
        }
        const TOTAL: usize = 300000;
        // const TOTAL: usize = 1000;
        tools::wait_all_hart();

        let push = |a| {
            TEST_QUEUE_0.push(a).unwrap();
        };
        let pop = || TEST_QUEUE_0.pop().unwrap();
        let take_all = || TEST_QUEUE_0.take_all().unwrap();
        for i in 0..100000 {
            if hart == 0 {
                println!("{}test {}", tools::n_space(off), i);
            }
            // group_test_impl(hart, 2, 2, TOTAL, push, pop, off + 4);
            // gather_test_impl(hart, 1000, TOTAL, push, pop, off + 4);
            take_all_test(hart, TOTAL, push, pop, take_all, off);
            tools::wait_all_hart();
        }
        tools::wait_all_hart();
        if hart == 0 {
            println!("{}lock free stack stress_test pass", tools::n_space(off));
        }
    }
}
