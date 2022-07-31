use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

/// 使用Arena方式来在固定内存区域运行动态异步任务并绕过动态内存分配
///
/// 如何在trait中定义一个动态的异步函数? 标准方法是使用`Pin<Box<dyn Future>>`
///
/// 但是Box分配内存太慢怎么办? 使用arena方法, 划出一块内存给trait的异步任务使用
///
/// 此方法有arena空间不足的风险, 当空间不足时将panic.
pub struct ArenaFuture<'a, T> {
    future: &'a mut (dyn Future<Output = T> + Send + 'a),
    drop_fn: fn(*mut ()),
}

impl<T> Drop for ArenaFuture<'_, T> {
    fn drop(&mut self) {
        (self.drop_fn)(self.future as *mut dyn Future<Output = T> as *mut _)
    }
}

impl<'a, T> ArenaFuture<'a, T> {
    /// 生成一个可以运行在动态上下文的Future
    /// ```no_run
    /// use ftl_util::async_tools::arena::ArenaFuture;
    /// 
    /// pub fn arena_test(buf: &mut [usize]) -> ArenaFuture<usize> {
    ///     ArenaFuture::new(buf, async move { 1 })
    /// }
    /// pub async fn arena_test_a(buf: &mut [usize]) -> usize {
    ///     ArenaFuture::new(buf, async move { 1 }).await
    /// }
    /// ```
    #[inline]
    pub fn new<A>(buf: &'a mut [A], f: impl Future<Output = T> + Send + 'a) -> Self {
        let future = unsafe { &mut *arena_split(buf).unwrap().0 };
        *future = f;
        let drop_fn = take_drop_fn(future);
        Self { future, drop_fn }
    }
    /// 生成一个可以运行在动态上下文的Future并继续使用同一块缓存的剩余部分
    /// ```no_run
    /// use ftl_util::async_tools::arena::ArenaFuture;
    /// 
    /// pub fn arena_test(buf: &mut [usize]) -> ArenaFuture<usize> {
    ///     ArenaFuture::new_with_buf(buf, |buf| async move {
    ///         ArenaFuture::new(buf, async move { 1 }).await;
    ///         1
    ///     })
    /// }
    /// ```
    #[inline]
    pub fn new_with_buf<A, F: Future<Output = T> + Send + 'a>(
        buf: &'a mut [A],
        f: impl FnOnce(&'a mut [A]) -> F,
    ) -> Self {
        let (future, next) = unsafe { arena_split(buf).unwrap() };
        *future = f(next);
        let drop_fn = take_drop_fn(future);
        Self { future, drop_fn }
    }
}

impl<T> Future for ArenaFuture<'_, T> {
    type Output = T;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        unsafe { Pin::new_unchecked(&mut *self.get_unchecked_mut().future).poll(cx) }
    }
}

/// 将内存对齐并划分为两半, 丢弃未对齐部分
unsafe fn arena_split<T, U>(buf: &mut [T]) -> Result<(&mut U, &mut [T]), ()> {
    let size = core::mem::size_of::<U>();
    let n_usize = size.div_ceil(core::mem::size_of::<T>());
    let offset = buf.as_ptr().align_offset(core::mem::align_of::<U>());
    if offset >= buf.len() {
        return Err(());
    }
    let buf = &mut buf[offset..];
    if buf.len() < n_usize {
        return Err(());
    }
    let (a, b) = buf.split_at_mut(n_usize);
    Ok((&mut *(a.as_ptr() as *mut U), b))
}

fn take_drop_fn<T, F: Future<Output = T>>(_: &F) -> fn(*mut ()) {
    |a| unsafe { core::ptr::drop_in_place::<F>(a as *mut _) }
}

mod test {
    //! 测试rust借用检查器是否有效
    #![allow(dead_code)]
    use super::ArenaFuture;

    pub fn arena_async(buf: &mut [usize]) -> ArenaFuture<usize> {
        ArenaFuture::new(buf, async move { 1usize })
    }

    pub fn arena_async2(buf: &mut [usize]) -> ArenaFuture<usize> {
        ArenaFuture::new_with_buf(buf, |buf| async move {
            ArenaFuture::new(buf, async move { 1 }).await;
            1
        })
    }

    pub fn arena_async_ref<'a, 'b>(
        buf: &'a mut [usize],
        b: &'b mut usize,
    ) -> ArenaFuture<'a, &'b mut usize> {
        ArenaFuture::new(buf, async move { b })
    }

    pub fn arena_async2_ref(buf: &mut [usize]) -> ArenaFuture<usize> {
        ArenaFuture::new_with_buf(buf, |buf| async move {
            let a = &mut 2;
            let _b = ArenaFuture::new(buf, async move { a }).await;
            1
        })
    }

    pub fn arena_async3_ref<'a>(buf: &'a mut [usize], a: &'a mut usize) -> ArenaFuture<'a, usize> {
        ArenaFuture::new_with_buf(buf, |buf| async move {
            let _b = ArenaFuture::new(buf, async move { a }).await;
            1
        })
    }
}
