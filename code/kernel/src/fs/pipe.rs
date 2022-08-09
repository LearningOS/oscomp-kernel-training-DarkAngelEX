use core::{
    future::Future,
    pin::Pin,
    sync::atomic::{AtomicUsize, Ordering},
    task::{Context, Poll, Waker},
};

use alloc::{
    boxed::Box,
    sync::{Arc, Weak},
};
use ftl_util::{
    async_tools::{self, ASysRet},
    error::{SysError, SysRet},
    fs::Seek,
};
use vfs::{
    select::{SelectNode, SelectSet, PL},
    File,
};

use crate::{
    config::PAGE_SIZE,
    local,
    memory::allocator::frame::{self, global::FrameTracker},
    sync::{
        even_bus::{self, Event},
        mutex::SpinLock,
        SleepMutex,
    },
    tools::{container::sync_unsafe_cell::SyncUnsafeCell, error::FrameOOM},
};

const RING_PAGE: usize = 4;
const RING_SIZE: usize = RING_PAGE * PAGE_SIZE;

/// 可以并行读写的管道，但禁止并行读/并行写。
pub struct Pipe {
    buffer: [FrameTracker; RING_PAGE],
    read_at: AtomicUsize,  // only modify by reader
    write_at: AtomicUsize, // only modify by writer
}

impl Pipe {
    pub fn new() -> Result<Self, FrameOOM> {
        Ok(Self {
            buffer: frame::global::alloc_n()?,
            read_at: AtomicUsize::new(0),
            write_at: AtomicUsize::new(0),
        })
    }
    pub fn max_read(&self) -> usize {
        (self.write_at.load(Ordering::Relaxed) + RING_SIZE - self.read_at.load(Ordering::Relaxed))
            % RING_SIZE
    }
    pub fn max_write(&self) -> usize {
        (self.read_at.load(Ordering::Relaxed) + RING_SIZE
            - 1
            - self.write_at.load(Ordering::Relaxed))
            % RING_SIZE
    }
    pub fn can_read(&self) -> bool {
        self.max_read() != 0
    }
    pub fn can_write(&self) -> bool {
        self.max_write() != 0
    }
    pub fn get_range(&mut self, at: usize, len: usize) -> &mut [u8] {
        let n = at % RING_SIZE / PAGE_SIZE;
        let i = at % PAGE_SIZE;
        let end = (i + len).min(PAGE_SIZE);
        &mut self.buffer[n].data().as_bytes_array_mut()[i..end]
    }
    /// never return zero, otherwise panic.
    pub fn read(&mut self, buffer: &mut [u8], mut wake_writer: impl FnMut()) -> usize {
        stack_trace!();
        let max = self.max_read();
        let read_at = self.read_at.load(Ordering::Acquire);
        let len = buffer.len().min(max);
        assert!(len != 0);
        let mut cur = 0;
        while cur < len {
            let ran = self.get_range(read_at + cur, len - cur);
            let n = ran.len();
            buffer[cur..cur + n].copy_from_slice(ran);
            cur += n;
            self.read_at
                .store((read_at + cur) % RING_SIZE, Ordering::Release);
            wake_writer();
        }
        assert!(cur == len);
        len
    }
    /// never return zero, otherwise panic.
    pub fn write(&mut self, buffer: &[u8], mut wake_reader: impl FnMut()) -> usize {
        stack_trace!();
        let max = self.max_write();
        let write_at = self.write_at.load(Ordering::Acquire);
        let len = buffer.len().min(max);
        assert!(len != 0);
        let mut cur = 0;
        while cur < len {
            let ran = self.get_range(write_at + cur, len - cur);
            let n = ran.len();
            ran.copy_from_slice(&buffer[cur..cur + n]);
            cur += n;
            self.write_at
                .store((write_at + cur) % RING_SIZE, Ordering::Release);
            wake_reader();
        }
        assert!(cur == len);
        len
    }
}

pub fn make_pipe() -> Result<(Arc<PipeReader>, Arc<PipeWriter>), FrameOOM> {
    let pipe = Arc::new(SyncUnsafeCell::new(Pipe::new()?));
    let mut reader = Arc::new(PipeReader {
        pipe: SleepMutex::new(pipe.clone()),
        writer: Weak::new(),
        waker: SpinLock::new(None),
        select_set: SpinLock::new(SelectSet::new()),
    });
    let writer = Arc::new(PipeWriter {
        pipe: SleepMutex::new(pipe),
        reader: Arc::downgrade(&reader),
        waker: SpinLock::new(None),
        select_set: SpinLock::new(SelectSet::new()),
    });

    unsafe {
        reader.select_set.unsafe_get_mut().init();
        writer.select_set.unsafe_get_mut().init();
        Arc::get_mut_unchecked(&mut reader).writer = Arc::downgrade(&writer);
    }
    Ok((reader, writer))
}

pub struct PipeReader {
    pipe: SleepMutex<Arc<SyncUnsafeCell<Pipe>>>,
    writer: Weak<PipeWriter>,
    waker: SpinLock<Option<Waker>>,
    select_set: SpinLock<SelectSet>,
}

impl Drop for PipeReader {
    fn drop(&mut self) {
        if let Some(w) = self.writer.upgrade().and_then(|w| w.waker.lock().take()) {
            w.wake()
        }
    }
}

impl File for PipeReader {
    fn readable(&self) -> bool {
        true
    }
    fn writable(&self) -> bool {
        false
    }
    fn lseek(&self, _offset: isize, _whence: Seek) -> SysRet {
        Err(SysError::ESPIPE)
    }
    fn read_fast(&self, buffer: &mut [u8]) -> SysRet {
        stack_trace!();
        if buffer.is_empty() {
            return Ok(0);
        }
        if unsafe { self.pipe.unsafe_get().get().max_read() < buffer.len() } {
            return Err(SysError::EAGAIN);
        }
        let pipe = self.pipe.try_lock().ok_or(SysError::EAGAIN)?;
        let pipe = unsafe { pipe.get() };
        if pipe.max_write() < buffer.len() {
            return Err(SysError::EAGAIN);
        }
        let n = pipe.read(buffer, wake_writer(&self.writer));
        debug_assert!(n == buffer.len());
        Ok(n)
    }
    fn read<'a>(&'a self, buffer: &'a mut [u8]) -> ASysRet {
        Box::pin(async move {
            stack_trace!();
            if buffer.is_empty() {
                return Ok(0);
            }
            let pipe = self.pipe.lock().await;
            let future = &mut ReadPipeFuture {
                pipe: unsafe { pipe.get() },
                waker: &self.waker,
                writer: &self.writer,
                buffer,
                current: 0,
            };
            future.init().await;
            // return future.await;
            let bus = &local::task_local().thread.process.event_bus;
            let waker = async_tools::take_waker().await;
            let event_future = even_bus::wait_for_event(bus, Event::RECEIVE_SIGNAL, &waker);
            match async_tools::Join2Future(future, event_future).await {
                async_tools::Join2R::First(r) => r,
                async_tools::Join2R::Second(_e) => Err(SysError::EINTR),
            }
        })
    }
    fn write<'a>(&'a self, _read_only: &'a [u8]) -> ASysRet {
        panic!("write to PipeReader");
    }
    fn ppoll(&self) -> PL {
        unsafe {
            if self.pipe.unsafe_get().get().can_read() {
                return PL::POLLIN;
            }
        }
        if self.writer.strong_count() == 0 {
            return PL::POLLPRI | PL::POLLHUP;
        }
        PL::empty()
    }
    fn push_select_node(&self, node: &mut SelectNode) {
        self.select_set.lock().push(node)
    }
    fn pop_select_node(&self, node: &mut SelectNode) {
        self.select_set.lock().pop(node)
    }
}

pub struct PipeWriter {
    pipe: SleepMutex<Arc<SyncUnsafeCell<Pipe>>>,
    reader: Weak<PipeReader>,
    waker: SpinLock<Option<Waker>>,
    select_set: SpinLock<SelectSet>,
}

impl Drop for PipeWriter {
    fn drop(&mut self) {
        if let Some(reader) = self.reader.upgrade() {
            if let Some(w) = reader.waker.lock().take() {
                w.wake();
            }
        }
    }
}

impl File for PipeWriter {
    fn readable(&self) -> bool {
        false
    }
    fn writable(&self) -> bool {
        true
    }
    fn lseek(&self, _offset: isize, _whence: Seek) -> SysRet {
        Err(SysError::ESPIPE)
    }
    fn read<'a>(&'a self, _write_only: &'a mut [u8]) -> ASysRet {
        panic!("read from PipeWriter");
    }
    fn write_fast(&self, buffer: &[u8]) -> SysRet {
        stack_trace!();
        if buffer.is_empty() {
            return Ok(0);
        }
        if unsafe { self.pipe.unsafe_get().get().max_write() < buffer.len() } {
            return Err(SysError::EAGAIN);
        }
        stack_trace!();
        let pipe = self.pipe.try_lock().ok_or(SysError::EAGAIN)?;
        let pipe = unsafe { pipe.get() };
        if pipe.max_write() < buffer.len() {
            return Err(SysError::EAGAIN);
        }
        stack_trace!();
        let n = pipe.write(buffer, wake_reader(&self.reader));
        debug_assert!(n == buffer.len());
        Ok(n)
    }
    fn write<'a>(&'a self, buffer: &'a [u8]) -> ASysRet {
        Box::pin(async move {
            stack_trace!();
            if buffer.is_empty() {
                return Ok(0);
            }
            let pipe = self.pipe.lock().await;
            let future = &mut WritePipeFuture {
                pipe: unsafe { pipe.get() },
                waker: &self.waker,
                reader: &self.reader,
                buffer,
                current: 0,
            };
            future.init().await;
            // return future.await;
            let bus = &local::task_local().thread.process.event_bus;
            let waker = async_tools::take_waker().await;
            let event_future = even_bus::wait_for_event(bus, Event::RECEIVE_SIGNAL, &waker);
            match async_tools::Join2Future(future, event_future).await {
                async_tools::Join2R::First(r) => r,
                async_tools::Join2R::Second(_e) => Err(SysError::EINTR),
            }
        })
    }
    fn ppoll(&self) -> PL {
        unsafe {
            if self.pipe.unsafe_get().get().can_write() {
                return PL::POLLOUT;
            }
        }
        if self.reader.strong_count() == 0 {
            return PL::POLLPRI | PL::POLLERR;
        }
        PL::empty()
    }
    fn push_select_node(&self, node: &mut SelectNode) {
        self.select_set.lock().push(node)
    }
    fn pop_select_node(&self, node: &mut SelectNode) {
        self.select_set.lock().pop(node)
    }
}

struct ReadPipeFuture<'a> {
    pipe: &'a mut Pipe,
    waker: &'a SpinLock<Option<Waker>>,
    writer: &'a Weak<PipeWriter>,
    buffer: &'a mut [u8],
    current: usize,
}

impl ReadPipeFuture<'_> {
    async fn init(&mut self) {
        let waker = ftl_util::async_tools::take_waker().await;
        self.waker.lock().replace(waker);
    }
    fn pbw(&mut self) -> (&'_ mut Pipe, &'_ mut [u8], impl FnMut() + '_) {
        (self.pipe, self.buffer, wake_writer(self.writer))
    }
}

impl Future for ReadPipeFuture<'_> {
    type Output = SysRet;
    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            stack_trace!();
            if self.current == self.buffer.len() {
                return Poll::Ready(Ok(self.current));
            }
            assert!(self.current < self.buffer.len());
            if !self.pipe.can_read() {
                match self.writer.strong_count() {
                    0 => return Poll::Ready(Ok(self.current)),
                    _ => match self.current {
                        0 => return Poll::Pending,
                        _ => return Poll::Ready(Ok(self.current)),
                    },
                };
            }
            let current = self.current;
            let (pipe, buffer, wake_writer) = self.pbw();
            let dst = &mut buffer[current..];
            self.current += pipe.read(dst, wake_writer);
        }
    }
}

struct WritePipeFuture<'a> {
    pipe: &'a mut Pipe,
    waker: &'a SpinLock<Option<Waker>>,
    reader: &'a Weak<PipeReader>,
    buffer: &'a [u8],
    current: usize,
}

impl WritePipeFuture<'_> {
    async fn init(&mut self) {
        let waker = ftl_util::async_tools::take_waker().await;
        self.waker.lock().replace(waker);
    }
    fn pbw(&mut self) -> (&'_ mut Pipe, &'_ [u8], impl FnMut() + '_) {
        (self.pipe, self.buffer, wake_reader(self.reader))
    }
}

impl Future for WritePipeFuture<'_> {
    type Output = SysRet;

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            stack_trace!();
            if self.current == self.buffer.len() {
                return Poll::Ready(Ok(self.current));
            }
            assert!(self.current < self.buffer.len());
            if !self.pipe.can_write() {
                match self.reader.strong_count() {
                    0 => return Poll::Ready(Err(SysError::EPIPE)),
                    _ => return Poll::Pending,
                };
            }
            let current = self.current;
            let (pipe, buffer, wake_reader) = self.pbw();
            let dst = &buffer[current..];
            self.current += pipe.write(dst, wake_reader);
        }
    }
}

fn wake_writer(writer: &Weak<PipeWriter>) -> impl FnMut() + '_ {
    || {
        stack_trace!();
        let _: Option<_> = try {
            let writer = writer.upgrade()?;
            writer.select_set.lock().wake(PL::POLLOUT);
            writer.waker.lock().as_ref()?.wake_by_ref();
            Some(())
        };
    }
}

fn wake_reader(reader: &Weak<PipeReader>) -> impl FnMut() + '_ {
    || {
        stack_trace!();
        let _: Option<_> = try {
            let reader = reader.upgrade()?;
            reader.select_set.lock().wake(PL::POLLIN);
            reader.waker.lock().as_ref()?.wake_by_ref();
            Some(()) // 为了让愚蠢的rust-analyzer不爆红
        };
    }
}
