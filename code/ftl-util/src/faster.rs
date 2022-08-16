use core::sync::atomic::{AtomicPtr, AtomicU32, AtomicUsize, Ordering};

/// 这个函数比copy_from_slice快! 因为copy_from_slice每次循环只会复制8字节, 具有更大的循环开销.
#[inline(never)]
pub fn page_copy(dst: &mut [usize; 512], src: &[usize; 512]) {
    for i in 0..64 {
        dst[i * 8 + 0] = src[i * 8 + 0];
        dst[i * 8 + 1] = src[i * 8 + 1];
        dst[i * 8 + 2] = src[i * 8 + 2];
        dst[i * 8 + 3] = src[i * 8 + 3];
        dst[i * 8 + 4] = src[i * 8 + 4];
        dst[i * 8 + 5] = src[i * 8 + 5];
        dst[i * 8 + 6] = src[i * 8 + 6];
        dst[i * 8 + 7] = src[i * 8 + 7];
    }
}

#[inline(never)]
pub fn huge_copy(mut dst: &mut [usize], mut src: &[usize]) {
    assert!(dst.len() == src.len());
    assert!(dst.len() >= 512);
    let mut n = dst.len();
    // 对齐到cache_line
    unsafe {
        loop {
            page_copy(
                core::mem::transmute(dst.as_mut_ptr()),
                core::mem::transmute(src.as_ptr()),
            );
            dst = &mut dst[512..];
            src = &src[512..];
            n -= 512;
            if n < 512 {
                break;
            }
        }
        if n != 0 {
            dst.copy_from_slice(src);
        }
        return;
    }
}

/// 自动根据切片长度判断用什么版本的复制
#[inline(always)]
pub fn u8copy(dst: &mut [u8], src: &[u8]) {
    #[inline(never)]
    #[cold]
    fn u8_fail() -> ! {
        panic!("u8 copy len check fail");
    }
    if dst.len() != src.len() {
        u8_fail();
    }
    const USIZE_SIZE: usize = core::mem::size_of::<usize>();
    let len = dst.len();
    unsafe {
        if len < 4096
            || dst.as_ptr() as usize % USIZE_SIZE != 0
            || src.as_ptr() as usize % USIZE_SIZE != 0
        {
            return core::ptr::copy_nonoverlapping(src.as_ptr(), dst.as_mut_ptr(), len);
        }
        huge_copy(
            core::slice::from_raw_parts_mut(dst.as_mut_ptr() as _, len / USIZE_SIZE),
            core::slice::from_raw_parts(src.as_ptr() as _, len / USIZE_SIZE),
        )
    }
}

/// 无锁多核复制系统, 每个CPU都保有一个, 将页复制开销分摊给其他的CPU
///
/// 推荐CPU每次可以协助复制1KB的数据, 耗时128个时钟,
/// 4个CPU同时争抢时达到最高吞吐量
///
/// 此设施不能在中断上下文使用, 但允许运行时发生中断, 窃取方发生中断会降低速度
pub struct SmpCopy {
    cur_end: AtomicU32, // 当前位置和结束位置
    wait: AtomicU32,    // 等待写入的单元数量, 变为0之前禁止init
    cell: AtomicUsize,  // 每个单元的大小
    src: AtomicPtr<usize>,
    dst: AtomicPtr<usize>,
}

impl SmpCopy {
    pub const fn new() -> Self {
        Self {
            cur_end: AtomicU32::new(0),
            wait: AtomicU32::new(0),
            cell: AtomicUsize::new(0),
            src: AtomicPtr::new(core::ptr::null_mut()),
            dst: AtomicPtr::new(core::ptr::null_mut()),
        }
    }
    fn cur_end(&self) -> u32 {
        self.cur_end.load(Ordering::Relaxed)
    }
    fn cur_end_fn(ce: u32) -> (u32, u32) {
        (ce >> 16, ce & ((1 << 16) - 1))
    }
    /// n 是切分数量, 如果为1就是独占
    pub fn init(&self, dst: &mut [usize], src: &[usize], n: usize) {
        debug_assert!(dst.len() == src.len());
        debug_assert!(!self.can_run());
        debug_assert!(n < u32::MAX as usize);
        debug_assert!(dst.len() % n == 0);
        let cell = dst.len() / n;
        self.cell.store(cell, Ordering::Relaxed);
        self.wait.store(n as u32, Ordering::Relaxed);
        self.src.store(src.as_ptr() as *mut _, Ordering::Relaxed);
        self.dst.store(dst.as_mut_ptr(), Ordering::Relaxed);
        self.cur_end.store(n as u32, Ordering::Release);
    }
    /// 在下一次init执行之前必须执行一次wait, 不能同时提交两个申请
    pub fn wait(&self) {
        while self.wait.load(Ordering::Acquire) != 0 {
            self.run();
        }
    }
    fn cas_set_cur_end(&self, old: u32, cur: u32, end: u32) -> bool {
        self.cur_end
            .compare_exchange(old, cur << 16 | end, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
    }
    pub fn can_run(&self) -> bool {
        let (cur, end) = Self::cur_end_fn(self.cur_end());
        debug_assert!(cur <= end);
        cur != end
    }
    // 尝试为这个执行器加速, 如果执行成功了返回true
    pub fn run(&self) -> bool {
        let cur = loop {
            let old = self.cur_end();
            let (cur, end) = Self::cur_end_fn(old);
            if cur == end {
                return false;
            }
            debug_assert!(cur < end);
            if !self.cas_set_cur_end(old, cur + 1, end) {
                continue;
            }
            break cur as usize;
        };
        debug_assert!(self.wait.load(Ordering::Relaxed) != 0);
        let cell = self.cell.load(Ordering::Relaxed);
        let src = self.src.load(Ordering::Relaxed).wrapping_add(cur * cell);
        let dst = self.dst.load(Ordering::Relaxed).wrapping_add(cur * cell);
        unsafe { core::ptr::copy_nonoverlapping(src, dst, cell) };
        self.wait.fetch_sub(1, Ordering::Release);
        true
    }
}
