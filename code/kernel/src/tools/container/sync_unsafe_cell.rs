pub struct SyncUnsafeCell<T> {
    data: T,
}
unsafe impl<T> Send for SyncUnsafeCell<T> {}
unsafe impl<T> Sync for SyncUnsafeCell<T> {}

impl<T> SyncUnsafeCell<T> {
    pub const fn new(data: T) -> Self {
        Self { data }
    }
    pub unsafe fn get(&self) -> &mut T {
        &mut *(&self.data as *const T as *mut T)
    }
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.data
    }
}
