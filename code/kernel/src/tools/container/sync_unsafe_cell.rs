use core::cell::UnsafeCell;

pub struct SyncUnsafeCell<T> {
    data: UnsafeCell<T>,
}
unsafe impl<T> Send for SyncUnsafeCell<T> {}
unsafe impl<T> Sync for SyncUnsafeCell<T> {}

impl<T> SyncUnsafeCell<T> {
    #[inline(always)]
    pub const fn new(data: T) -> Self {
        Self { data: UnsafeCell::new(data) }
    }
    #[inline(always)]
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn get(&self) -> &mut T {
        &mut *self.data.get()
    }
    #[inline(always)]
    pub fn get_mut(&mut self) -> &mut T {
        self.data.get_mut()
    }
}
