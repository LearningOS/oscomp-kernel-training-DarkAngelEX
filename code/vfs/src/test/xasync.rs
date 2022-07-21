use core::{
    cell::SyncUnsafeCell,
    future::Future,
    marker::PhantomData,
    mem::ManuallyDrop,
    ops::Deref,
    task::{Context, RawWaker, RawWakerVTable, Waker},
};

use alloc::{boxed::Box, collections::VecDeque, sync::Arc};
use ftl_util::{
    async_tools::Async,
    sync::{spin_mutex::SpinMutex, Spin},
};

/// 一个Future，它可以调度自己(将自己放入任务通道中)，然后等待执行器去`poll`
struct Task {
    future: SyncUnsafeCell<Option<Async<'static, ()>>>,
    /// 可以将该任务自身放回到任务通道中，等待执行器的poll
    task_sender: SyncSender<Arc<Task>>,
}
unsafe impl Sync for Task {}

/// 任务执行器，负责从通道中接收任务然后执行
pub struct Executor {
    ready_queue: Receiver<Arc<Task>>,
}

/// `Spawner`负责创建新的`Future`然后将它发送到任务通道中
#[derive(Clone)]
pub struct Spawner {
    task_sender: SyncSender<Arc<Task>>,
}

pub fn new_executor_and_spawner() -> (Executor, Spawner) {
    // 任务通道允许的最大缓冲数(任务队列的最大长度)
    // 当前的实现仅仅是为了简单，在实际的执行中，并不会这么使用
    const MAX_QUEUED_TASKS: usize = 10_000;
    let (task_sender, ready_queue) = sync_channel(MAX_QUEUED_TASKS);
    (Executor { ready_queue }, Spawner { task_sender })
}

impl Spawner {
    pub fn spawn(&self, future: impl Future<Output = ()> + Send + 'static) {
        let future = Box::pin(future);
        let task = Arc::new(Task {
            future: SyncUnsafeCell::new(Some(future)),
            task_sender: self.task_sender.clone(),
        });
        self.task_sender.send(task).expect("任务队列已满");
    }
}

impl ArcWake for Task {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        println!("task wake!");
        // 通过发送任务到任务管道的方式来实现`wake`，这样`wake`后，任务就能被执行器`poll`
        let cloned = arc_self.clone();
        arc_self.task_sender.send(cloned).expect("任务队列已满");
    }
    fn wake(self: Arc<Self>) {
        Self::wake_by_ref(&self)
    }
}

impl Executor {
    pub fn run(&self) {
        while let Ok(task) = self.ready_queue.recv() {
            let future_slot = unsafe { &mut *task.future.get() };
            if let Some(mut future) = future_slot.take() {
                let waker = waker_ref(&task);
                let context = &mut Context::from_waker(&*waker);
                if future.as_mut().poll(context).is_pending() {
                    *future_slot = Some(future);
                }
            }
        }
    }
    pub fn run_debug(&self) {
        println!("run begin");
        while let Ok(task) = self.ready_queue.recv() {
            println!("fetch");
            // 获取一个future，若它还没有完成(仍然是Some，不是None)，则对它进行一次poll并尝试完成它
            let future_slot = unsafe { &mut *task.future.get() };
            if let Some(mut future) = future_slot.take() {
                // 基于任务自身创建一个 `LocalWaker`
                let waker = waker_ref(&task);
                let context = &mut Context::from_waker(&*waker);
                // `BoxFuture<T>`是`Pin<Box<dyn Future<Output = T> + Send + 'static>>`的类型别名
                // 通过调用`as_mut`方法，可以将上面的类型转换成`Pin<&mut dyn Future + Send + 'static>`
                if future.as_mut().poll(context).is_pending() {
                    // Future还没执行完，因此将它放回任务中，等待下次被poll
                    *future_slot = Some(future);
                }
            }
        }
        println!("run end");
    }
}

// =============================================================================================
// ======================================== SEND - RECV ========================================
// =============================================================================================

struct SyncBuffer<T>(VecDeque<T>);

#[derive(Clone)]
struct SyncSender<T>(Arc<SpinMutex<SyncBuffer<T>, Spin>>);
impl<T> SyncSender<T> {
    pub fn send(&self, v: T) -> Result<(), ()> {
        self.0.lock().0.push_back(v);
        Ok(())
    }
}

struct Receiver<T>(Arc<SpinMutex<SyncBuffer<T>, Spin>>);
impl<T> Receiver<T> {
    pub fn recv(&self) -> Result<T, ()> {
        self.0.lock().0.pop_front().ok_or(())
    }
}

fn sync_channel<T>(_max: usize) -> (SyncSender<T>, Receiver<T>) {
    let buffer = Arc::new(SpinMutex::new(SyncBuffer(VecDeque::new())));
    (SyncSender(buffer.clone()), Receiver(buffer))
}

// =============================================================================================
// ======================================== RUST STD LIB =======================================
// =============================================================================================

pub struct WakerRef<'a> {
    waker: ManuallyDrop<Waker>,
    _marker: PhantomData<&'a ()>,
}

impl<'a> WakerRef<'a> {
    pub fn new(waker: &'a Waker) -> Self {
        let waker = ManuallyDrop::new(unsafe { core::ptr::read(waker) });
        Self {
            waker,
            _marker: PhantomData,
        }
    }
    pub fn new_unowned(waker: ManuallyDrop<Waker>) -> Self {
        Self {
            waker,
            _marker: PhantomData,
        }
    }
}

impl Deref for WakerRef<'_> {
    type Target = Waker;
    fn deref(&self) -> &Waker {
        &self.waker
    }
}

#[inline]
fn waker_ref<W>(wake: &Arc<W>) -> WakerRef<'_>
where
    W: ArcWake,
{
    let ptr = Arc::as_ptr(wake).cast::<()>();

    let waker =
        ManuallyDrop::new(unsafe { Waker::from_raw(RawWaker::new(ptr, waker_vtable::<W>())) });
    WakerRef::new_unowned(waker)
}

fn waker_vtable<W: ArcWake>() -> &'static RawWakerVTable {
    &RawWakerVTable::new(
        clone_arc_raw::<W>,
        wake_arc_raw::<W>,
        wake_by_ref_arc_raw::<W>,
        drop_arc_raw::<W>,
    )
}

fn waker<W>(wake: Arc<W>) -> Waker
where
    W: ArcWake + 'static,
{
    let ptr = Arc::into_raw(wake).cast::<()>();

    unsafe { Waker::from_raw(RawWaker::new(ptr, waker_vtable::<W>())) }
}

#[allow(clippy::redundant_clone)] // The clone here isn't actually redundant.
unsafe fn increase_refcount<T: ArcWake>(data: *const ()) {
    let arc = ManuallyDrop::new(Arc::<T>::from_raw(data.cast::<T>()));
    let _arc_clone: ManuallyDrop<_> = arc.clone();
}

// used by `waker_ref`
unsafe fn clone_arc_raw<T: ArcWake>(data: *const ()) -> RawWaker {
    increase_refcount::<T>(data);
    RawWaker::new(data, waker_vtable::<T>())
}

unsafe fn wake_arc_raw<T: ArcWake>(data: *const ()) {
    let arc: Arc<T> = Arc::from_raw(data.cast::<T>());
    ArcWake::wake(arc);
}

// used by `waker_ref`
unsafe fn wake_by_ref_arc_raw<T: ArcWake>(data: *const ()) {
    // Retain Arc, but don't touch refcount by wrapping in ManuallyDrop
    let arc = ManuallyDrop::new(Arc::<T>::from_raw(data.cast::<T>()));
    ArcWake::wake_by_ref(&arc);
}

unsafe fn drop_arc_raw<T: ArcWake>(data: *const ()) {
    drop(Arc::<T>::from_raw(data.cast::<T>()))
}

trait ArcWake: Send + Sync {
    fn wake(self: Arc<Self>) {
        Self::wake_by_ref(&self)
    }
    fn wake_by_ref(arc_self: &Arc<Self>);
}
