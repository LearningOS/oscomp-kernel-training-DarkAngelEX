use core::{
    future::Future,
    pin::Pin,
    sync::atomic::{self, Ordering},
    task::{Context, Poll, Waker},
};

use alloc::{
    boxed::Box,
    sync::{Arc, Weak},
};

use crate::{
    config::PAGE_SIZE,
    memory::allocator::frame::{self, global::FrameTracker},
    sync::{mutex::SpinNoIrqLock, sleep_mutex::SleepMutex},
    syscall::SysResult,
    tools::{container::sync_unsafe_cell::SyncUnsafeCell, error::FrameOutOfMemory},
    user::{UserData, UserDataMut},
};

use super::{AsyncFile, File};

const RING_PAGE: usize = 4;
const RING_SIZE: usize = RING_PAGE * PAGE_SIZE;

/// 可以并行读写的管道，但读写者都至多同时存在一个。
pub struct Pipe {
    buffer: [FrameTracker; RING_PAGE],
    read_at: usize,  // only modify by reader
    write_at: usize, // only modify by writer
}

impl Pipe {
    pub fn new() -> Result<Self, FrameOutOfMemory> {
        Ok(Self {
            buffer: frame::global::alloc_n()?,
            read_at: 0,
            write_at: 0,
        })
    }
    pub fn max_read(&self) -> usize {
        (self.write_at + RING_SIZE - self.read_at) % RING_SIZE
    }
    pub fn max_write(&self) -> usize {
        (self.read_at + RING_SIZE - 1 - self.write_at) % RING_SIZE
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
        let end = PAGE_SIZE.min(i + len);
        &mut self.buffer[n].data().as_bytes_array_mut()[i..end]
    }
    /// never return zero, otherwise panic.
    pub fn read(&mut self, buffer: &mut [u8]) -> usize {
        let max = self.max_read();
        let read_at = self.read_at;
        let len = buffer.len().min(max);
        assert!(len != 0);
        atomic::fence(Ordering::Acquire);
        let mut cur = 0;
        while cur < len {
            let ran = self.get_range(read_at + cur, len - cur);
            let n = ran.len();
            buffer[cur..cur + n].copy_from_slice(ran);
            cur += n;
        }
        assert!(cur == len);
        atomic::fence(Ordering::Release);
        self.read_at = (read_at + len) % RING_SIZE;
        len
    }
    /// never return zero, otherwise panic.
    pub fn write(&mut self, buffer: &[u8]) -> usize {
        let max = self.max_write();
        let write_at = self.write_at;
        let len = buffer.len().min(max);
        assert!(len != 0);
        atomic::fence(Ordering::Acquire);
        let mut cur = 0;
        while cur < len {
            let ran = self.get_range(write_at + cur, len - cur);
            let n = ran.len();
            ran.copy_from_slice(&buffer[cur..cur + n]);
            cur += n;
        }
        assert!(cur == len);
        atomic::fence(Ordering::Release);
        self.write_at = (write_at + len) % RING_SIZE;
        len
    }
}

pub fn make_pipe() -> Result<(Arc<PipeReader>, Arc<PipeWriter>), FrameOutOfMemory> {
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
            .and_then(|w| w.waker.lock(place!()).take())
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
    fn read(self: Arc<Self>, write_only: UserDataMut<u8>) -> AsyncFile {
        Box::pin(async move {
            if write_only.len() == 0 {
                return Ok(0);
            }
            let pipe = self.pipe.lock().await;
            ReadPipeFuture {
                pipe: unsafe { pipe.get() },
                waker: &self.waker,
                writer: &self.writer,
                buffer: write_only,
                current: 0,
            }
            .await
        })
    }
    fn write(self: Arc<Self>, _read_only: UserData<u8>) -> AsyncFile {
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
            .and_then(|w| w.waker.lock(place!()).take())
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
    fn read(self: Arc<Self>, _write_only: UserDataMut<u8>) -> AsyncFile {
        panic!("read from PipeWriter");
    }
    fn write(self: Arc<Self>, read_only: UserData<u8>) -> AsyncFile {
        Box::pin(async move {
            if read_only.len() == 0 {
                return Ok(0);
            }
            let pipe = self.pipe.lock().await;
            WritePipeFuture {
                pipe: unsafe { pipe.get() },
                waker: &self.waker,
                reader: &self.reader,
                buffer: read_only,
                current: 0,
            }
            .await
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
}
impl Future for ReadPipeFuture<'_> {
    type Output = SysResult;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        macro_rules! pending {
            () => {{
                let prev = self.waker.lock(place!()).replace(cx.waker().clone());
                assert!(prev.is_none());
                Poll::Pending
            }};
        }

        assert_ne!(self.buffer.len(), self.current);
        if !self.pipe.can_read() {
            return match self.writer.upgrade() {
                Some(_w) => pending!(),
                None => Poll::Ready(Ok(self.current)),
            };
        }
        // let dst = &mut self.buffer.access_mut()[self.current..];
        // self.current += self.pipe.read(dst);
        let current = self.current;
        let (pipe, buffer) = self.pipe_and_buf_mut();
        let dst = &mut buffer.access_mut()[current..];
        self.current += pipe.read(dst);

        if let Some(w) = self
            .writer
            .upgrade()
            .and_then(|w| w.waker.lock(place!()).take())
        {
            w.wake()
        } else {
            return Poll::Ready(Ok(self.current));
        }
        if self.current == self.buffer.len() {
            Poll::Ready(Ok(self.current))
        } else {
            pending!()
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
}
impl Future for WritePipeFuture<'_> {
    type Output = SysResult;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        macro_rules! pending {
            () => {{
                let prev = self.waker.lock(place!()).replace(cx.waker().clone());
                assert!(prev.is_none());
                Poll::Pending
            }};
        }
        assert_ne!(self.buffer.len(), self.current);
        if !self.pipe.can_write() {
            return match self.reader.upgrade() {
                Some(_w) => pending!(),
                None => Poll::Ready(Ok(self.current)),
            };
        }
        // let dst = &self.buffer.access()[self.current..];
        // self.current += self.pipe.write(dst);
        let current = self.current;
        let (pipe, buffer) = self.pipe_and_buf_mut();
        let dst = &buffer.access()[current..];
        self.current += pipe.write(dst);

        if let Some(w) = self
            .reader
            .upgrade()
            .and_then(|w| w.waker.lock(place!()).take())
        {
            w.wake()
        } else {
            return Poll::Ready(Ok(self.current));
        }
        if self.current == self.buffer.len() {
            Poll::Ready(Ok(self.current))
        } else {
            pending!()
        }
    }
}
