/// 逻辑参考自 https://coolshell.cn/articles/8239.html
///
/// 无锁队列的优势除了无锁, 更重要的是使用时不需要关中断。
///
/// 压力测试中约1亿次操作出错一次，原因未知
use core::{mem::MaybeUninit, ptr::NonNull};

use alloc::boxed::Box;

use crate::tools::{container::thread_local_linked_list::ThreadLocalLinkedList, FailRun};

use super::marked_ptr::{AtomicMarkedPtr, MarkedPtr, PtrID};

const QUEUE_DEBUG: bool = true;
// 无锁单向链表
pub struct LockfreeQueue<T> {
    head: AtomicMarkedPtr<LockfreeNode<T>>,
    tail: AtomicMarkedPtr<LockfreeNode<T>>,
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
            debug_assert!(tail.get_ptr().is_none());
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
    /// 所有权操作! 细节见replace_impl
    pub fn take(&self) -> Result<ThreadLocalLinkedList<T>, ()> {
        stack_trace!();
        let mut new_dummy = Box::new(LockfreeNode::dummy());
        match self.replace_impl(new_dummy.as_mut(), new_dummy.as_mut()) {
            Ok(list) => {
                Box::leak(new_dummy);
                Ok(list)
            }
            Err(_) => Err(()),
        }
    }
    /// if error, return input list.
    pub fn relpace(
        &self,
        list: ThreadLocalLinkedList<T>,
    ) -> Result<ThreadLocalLinkedList<T>, ThreadLocalLinkedList<T>> {
        stack_trace!();
        let mut new_dummy = Box::new(LockfreeNode::dummy());
        let tail = match list.head_tail() {
            Some((head, tail)) => {
                new_dummy.next.init(head);
                tail
            }
            None => new_dummy.as_mut(),
        };
        match self.replace_impl(new_dummy.as_mut(), tail) {
            Ok(new_list) => {
                Box::leak(new_dummy);
                core::mem::forget(list);
                Ok(new_list)
            }
            Err(_) => Err(list),
        }
    }
    /// 非所有权操作, 可以被任意执行!
    ///
    /// 无竞争时可以O(1)修改tail指针! 但有竞争时可能导致tail更新失败, 导致其他进程缓慢地fetch tail到队尾, 但不会出错.
    pub fn appand(&self, list: &mut ThreadLocalLinkedList<T>) -> Result<(), ()> {
        stack_trace!();
        let (head, tail) = list.head_tail().ok_or(())?;
        match self.appand_impl(head.get_mut().unwrap(), tail) {
            Ok(_) => {
                unsafe { list.leak_reset() };
                Ok(())
            }
            Err(_) => Err(()),
        }
    }
    /// 所有权操作 任何所有权操作都是互斥的 但push/pop操作仍可以同时进行.
    fn replace_impl(
        &self,
        in_head: *mut LockfreeNode<T>,
        in_tail: *mut LockfreeNode<T>,
    ) -> Result<ThreadLocalLinkedList<T>, ()> {
        stack_trace!();
        debug_assert!(!in_head.is_null() && !in_tail.is_null());
        let mut tail = self.tail.load();
        loop {
            tail.get_ptr().ok_or(())?;
            let tail_next = &tail.get_mut().unwrap().next;
            let tail_next_v = tail_next.load();
            let cur_tail = self.tail.load();
            if tail != cur_tail {
                tail = cur_tail;
                continue;
            }
            unsafe { (*in_tail).next.set_id_null(tail.id()) };
            let new_tail = MarkedPtr::new(tail.id(), NonNull::new(in_tail));
            debug_assert!(tail.id().is_valid());
            debug_assert!(tail_next_v.id().is_valid());
            match self.tail.compare_exchange(tail, new_tail) {
                Ok(_) => {
                    // change id to disable the push of other threads.
                    let _ = tail_next.compare_exchange(tail_next_v, tail_next_v);
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

        let next_v = loop {
            // 禁止 take 函数发生时被另一个进程关闭
            head.get_ptr().unwrap();
            let head_next = &head.get_mut().unwrap().next;
            let next_v = head_next.load();
            let cur_head = self.head.load();
            if head != cur_head {
                head = cur_head;
                continue;
            }
            let new_head = MarkedPtr::new(head.id(), NonNull::new(in_head));
            debug_assert!(head.id().is_valid());
            match self.head.compare_exchange(head, new_head) {
                Ok(_) => {
                    let _ = head_next.compare_exchange(next_v, next_v);
                    break next_v;
                }
                Err(cur_head) => {
                    head = cur_head;
                    core::hint::spin_loop();
                    continue;
                }
            }
        };
        let list = ThreadLocalLinkedList::ptr_new(next_v.cast());
        unsafe { Box::from_raw(head.get_ptr().unwrap().as_ptr()) };
        Ok(list)
    }
    /// in_head is valid node, not dummy.
    fn appand_impl(
        &self,
        in_head: *mut LockfreeNode<T>,
        in_tail: *mut LockfreeNode<T>,
    ) -> Result<(), ()> {
        stack_trace!();
        debug_assert!(!in_head.is_null() && !in_tail.is_null());
        let mut new_node: NonNull<_> = NonNull::new(in_head).unwrap();
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
            unsafe { new_node.as_mut().next.set_id(tail.id()) };
            let new_tail = MarkedPtr::new(next_ptr.id(), Some(new_node));
            // block();
            // next 延迟修改会导致 take_all 无法正确获得最新 next
            match next.compare_exchange(next_ptr, new_tail) {
                Ok(_) => (),
                Err(_) => {
                    tail = self.tail.load();
                    core::hint::spin_loop();
                    continue;
                }
            }
            let new_tail = MarkedPtr::new(tail.id(), NonNull::new(in_tail));
            let _ = self.tail.compare_exchange(tail, new_tail);
            return Ok(());
        }
    }
    // 所有权操作
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
        stack_trace!();
        let node = LockfreeNode::new(value);
        let mut new_node: NonNull<_> = Box::leak(Box::new(node)).into();
        let fail_run = FailRun::new(move || unsafe {
            Box::from_raw(new_node.as_ptr());
        });
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
            let new_tail = MarkedPtr::new(next_ptr.id(), Some(new_node));
            // block();
            // next 延迟修改会导致 take_all 无法正确获得最新 next
            match next.compare_exchange(next_ptr, new_tail) {
                Ok(_) => (),
                Err(_) => {
                    tail = self.tail.load();
                    core::hint::spin_loop();
                    continue;
                }
            }
            let new_tail = MarkedPtr::new(tail.id(), Some(new_node));
            let _ = self.tail.compare_exchange(tail, new_tail);
            fail_run.consume();
            return Ok(());
        }
    }
    pub fn pop(&self) -> Result<Option<T>, ()> {
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
                Ok(_) => (),
                Err(cur_head) => {
                    head = cur_head;
                    core::hint::spin_loop();
                    continue;
                }
            }
            // 如果 CAS 成功则拥有了 head 与 value 的所有权. head 已经被非空判断.
            unsafe {
                let old_head = head.get_ptr().unwrap().as_ptr();
                // 释放内存
                Box::from_raw(old_head);
                // next become new dummy
                let value = value.assume_init_read();
                return Ok(Some(value));
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
    fn oper_test(
        hart: usize,
        total: usize,
        push: impl Fn(usize),
        pop: impl Fn() -> Option<usize>,
        take: impl Fn() -> ThreadLocalLinkedList<usize>,
        replace: impl Fn(ThreadLocalLinkedList<usize>) -> ThreadLocalLinkedList<usize>,
        appand: impl Fn(&mut ThreadLocalLinkedList<usize>),
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
                let test_take = false;
                let test_replace = true;
                if test_take {
                    stack_trace!();
                    const IN_ORDER: bool = false;
                    let mut xv = 0;
                    loop {
                        if COUNT_PUSH.load(Ordering::Relaxed) >= total {
                            break;
                        }
                        let mut list = take();
                        let mut cnt = 0;
                        while let Some(v) = list.pop() {
                            if IN_ORDER {
                                assert_eq!(xv, v);
                                xv += 1;
                            }
                            // print_unlock!("{}{} {}", to_green!(), v, reset_color!());
                            unsafe { SET_TABLE[hart].push(v) };
                            cnt += 1;
                        }
                        COUNT_POP.fetch_add(cnt, Ordering::Relaxed);
                    }
                } else if test_replace {
                    stack_trace!();
                    let mut list = ThreadLocalLinkedList::empty();
                    loop {
                        if COUNT_PUSH.load(Ordering::Relaxed) >= total {
                            break;
                        }
                        list = replace(list);
                        let len = list.len();
                        let cnt = len / 2;
                        for _i in 0..cnt {
                            let v = list.pop().unwrap();
                            unsafe { SET_TABLE[hart].push(v) };
                        }
                        COUNT_POP.fetch_add(cnt, Ordering::Relaxed);
                    }
                    let mut cnt = 0;
                    while let Some(v) = list.pop() {
                        unsafe { SET_TABLE[hart].push(v) };
                        cnt += 1;
                    }
                    COUNT_POP.fetch_add(cnt, Ordering::Relaxed);
                } else {
                    panic!();
                }
            }
            1 | 2 => loop {
                if COUNT_PUSH.load(Ordering::Relaxed) >= total {
                    break;
                }
                if let Some(v) = pop() {
                    // print_unlock!("{}{} {}", to_red!(), v, reset_color!());
                    unsafe { SET_TABLE[hart].push(v) };
                    COUNT_POP.fetch_add(1, Ordering::Relaxed);
                }
            },
            3 => {
                const BATCH: usize = 1000;
                loop {
                    let begin = COUNT_PUSH.fetch_add(BATCH, Ordering::Relaxed);
                    if begin >= total {
                        break;
                    }
                    let end = (begin + BATCH).min(total);
                    if true {
                        let mut list = ThreadLocalLinkedList::empty();
                        for i in begin..end {
                            list.push(i);
                        }
                        appand(&mut list);
                    } else {
                        for i in begin..end {
                            push(i);
                        }
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
            if false {
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
        let push = |a| TEST_QUEUE_1.lock().push_back(a);
        let pop = || TEST_QUEUE_1.lock().pop_front();
        group_test_impl(hart, 1, 1, TOTAL, push, pop, off + 4);
        group_test_impl(hart, 1, 3, TOTAL, push, pop, off + 4);
        group_test_impl(hart, 2, 2, TOTAL, push, pop, off + 4);
        group_test_impl(hart, 3, 1, TOTAL, push, pop, off + 4);
        if hart == 0 {
            println!("{}locked VecDeque test begin", tools::n_space(off));
        }
        let push = |a| {
            let v = &**TEST_QUEUE_2.lock();
            #[allow(clippy::cast_ref_to_mut)]
            unsafe { &mut *(v as *const _ as *mut VecDeque<usize>) }.push_back(a)
        };
        let pop = || {
            let v = &**TEST_QUEUE_2.lock();
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
        let take = || TEST_QUEUE_0.take().unwrap();
        let replace = |list| TEST_QUEUE_0.relpace(list).ok().unwrap();
        let appand = |list: &mut ThreadLocalLinkedList<usize>| TEST_QUEUE_0.appand(list).unwrap();
        for i in 0..100000 {
            if hart == 0 {
                println!("{}test {}", tools::n_space(off), i);
            }
            match 2 {
                0 => group_test_impl(hart, 2, 2, TOTAL, push, pop, off + 4),
                1 => gather_test_impl(hart, 1000, TOTAL, push, pop, off + 4),
                2 => oper_test(hart, TOTAL, push, pop, take, replace, appand, off),
                _ => panic!(),
            }
            tools::wait_all_hart();
        }
        tools::wait_all_hart();
        if hart == 0 {
            println!("{}lock free stack stress_test pass", tools::n_space(off));
        }
    }
}
