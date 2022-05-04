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
use ftl_util::error::SysError;

use crate::{
    config::PAGE_SIZE,
    memory::allocator::frame::{self, global::FrameTracker},
    sync::{mutex::SpinNoIrqLock, SleepMutex},
    syscall::SysResult,
    tools::{container::sync_unsafe_cell::SyncUnsafeCell, error::FrameOOM},
    user::{UserData, UserDataMut},
};

use super::{AsyncFile, File};

const RING_PAGE: usize = 4;
const RING_SIZE: usize = RING_PAGE * PAGE_SIZE;

/// 可以并行读写的管道，但读写者都至多同时存在一个。
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
    pub fn read(&mut self, buffer: &mut [u8]) -> usize {
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
                .store((read_at + cur) % RING_SIZE, Ordering::SeqCst);
        }
        assert!(cur == len);
        len
    }
    /// never return zero, otherwise panic.
    pub fn write(&mut self, buffer: &[u8]) -> usize {
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
                .store((write_at + cur) % RING_SIZE, Ordering::SeqCst);
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
        waker: SpinNoIrqLock::new(None),
    });
    let writer = Arc::new(PipeWriter {
        pipe: SleepMutex::new(pipe),
        reader: Arc::downgrade(&reader),
        waker: SpinNoIrqLock::new(None),
    });
    unsafe { Arc::get_mut_unchecked(&mut reader).writer = Arc::downgrade(&writer) };
    Ok((reader, writer))
}

pub struct PipeReader {
    pipe: SleepMutex<Arc<SyncUnsafeCell<Pipe>>>,
    writer: Weak<PipeWriter>,
    waker: SpinNoIrqLock<Option<Waker>>,
}

impl Drop for PipeReader {
    fn drop(&mut self) {
        if let Some(w) = self
            .writer
            .upgrade()
            .and_then(|w| w.waker.lock().take())
        {
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
    fn read(&self, write_only: UserDataMut<u8>) -> AsyncFile {
        Box::pin(async move {
            if write_only.len() == 0 {
                return Ok(0);
            }
            let pipe = self.pipe.lock().await;
            let mut future = ReadPipeFuture {
                pipe: unsafe { pipe.get() },
                waker: &self.waker,
                writer: &self.writer,
                buffer: write_only,
                current: 0,
            };
            future.init().await;
            future.await
        })
    }
    fn write(&self, _read_only: UserData<u8>) -> AsyncFile {
        panic!("write to PipeReader");
    }
}

pub struct PipeWriter {
    pipe: SleepMutex<Arc<SyncUnsafeCell<Pipe>>>,
    reader: Weak<PipeReader>,
    waker: SpinNoIrqLock<Option<Waker>>,
}

impl Drop for PipeWriter {
    fn drop(&mut self) {
        if let Some(w) = self
            .reader
            .upgrade()
            .and_then(|w| w.waker.lock().take())
        {
            w.wake()
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
    fn read(&self, _write_only: UserDataMut<u8>) -> AsyncFile {
        panic!("read from PipeWriter");
    }
    fn write(&self, read_only: UserData<u8>) -> AsyncFile {
        Box::pin(async move {
            if read_only.len() == 0 {
                return Ok(0);
            }
            let pipe = self.pipe.lock().await;
            let mut future = WritePipeFuture {
                pipe: unsafe { pipe.get() },
                waker: &self.waker,
                reader: &self.reader,
                buffer: read_only,
                current: 0,
            };
            future.init().await;
            future.await
        })
    }
}

struct ReadPipeFuture<'a> {
    pipe: &'a mut Pipe,
    waker: &'a SpinNoIrqLock<Option<Waker>>,
    writer: &'a Weak<PipeWriter>,
    buffer: UserDataMut<u8>,
    current: usize,
}
impl ReadPipeFuture<'_> {
    fn pipe_and_buf_mut(&mut self) -> (&'_ mut Pipe, &'_ mut UserDataMut<u8>) {
        (self.pipe, &mut self.buffer)
    }
    async fn init(&mut self) {
        let waker = ftl_util::async_tools::take_waker().await;
        self.waker.lock().replace(waker);
    }
}
impl Future for ReadPipeFuture<'_> {
    type Output = SysResult;
    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            if self.current == self.buffer.len() {
                return Poll::Ready(Ok(self.current));
            }
            assert!(self.current < self.buffer.len());
            if !self.pipe.can_read() {
                return match self.writer.upgrade() {
                    Some(_) => Poll::Pending,
                    None => Poll::Ready(Ok(self.current)),
                };
            }
            let current = self.current;
            let (pipe, buffer) = self.pipe_and_buf_mut();
            let dst = &mut buffer.access_mut()[current..];
            self.current += pipe.read(dst);

            self.writer
                .upgrade()
                .and_then(|w| w.waker.lock().as_ref().map(|w| w.wake_by_ref()));
        }
    }
}

struct WritePipeFuture<'a> {
    pipe: &'a mut Pipe,
    waker: &'a SpinNoIrqLock<Option<Waker>>,
    reader: &'a Weak<PipeReader>,
    buffer: UserData<u8>,
    current: usize,
}
impl WritePipeFuture<'_> {
    fn pipe_and_buf_mut(&mut self) -> (&'_ mut Pipe, &'_ mut UserData<u8>) {
        (self.pipe, &mut self.buffer)
    }
    async fn init(&mut self) {
        let waker = ftl_util::async_tools::take_waker().await;
        self.waker.lock().replace(waker);
    }
}
impl Future for WritePipeFuture<'_> {
    type Output = SysResult;

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            if self.current == self.buffer.len() {
                return Poll::Ready(Ok(self.current));
            }
            assert!(self.current < self.buffer.len());
            if !self.pipe.can_write() {
                return match self.reader.upgrade() {
                    Some(_) => Poll::Pending,
                    None => Poll::Ready(Err(SysError::EPIPE)),
                };
            }
            let current = self.current;
            let (pipe, buffer) = self.pipe_and_buf_mut();
            let dst = &buffer.access()[current..];
            self.current += pipe.write(dst);

            self.reader
                .upgrade()
                .and_then(|w| w.waker.lock().as_ref().map(|w| w.wake_by_ref()));
        }
    }
}
