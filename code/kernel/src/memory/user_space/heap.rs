use crate::memory::{
    address::{PageCount, UserAddr4K},
    page_table::PTEFlags,
    user_space::UserArea,
};

#[derive(Debug, Clone)]
pub struct HeapManager {
    heap_size: PageCount,
    heap_alloc: PageCount, // lazy alloc cnt
    heap_free: PageCount,  // free count
}
impl Drop for HeapManager {
    fn drop(&mut self) {
        assert!(self.heap_alloc == self.heap_free, "heap leak!");
    }
}
impl HeapManager {
    pub fn new() -> Self {
        Self {
            heap_size: PageCount::from_usize(0),
            heap_alloc: PageCount::from_usize(0),
            heap_free: PageCount::from_usize(0),
        }
    }
    pub fn size(&self) -> PageCount {
        self.heap_size
    }
    pub fn set_size_bigger(&mut self, new: PageCount) {
        assert!(new >= self.heap_size);
        self.heap_size = new;
    }
    pub fn set_size_smaller(&mut self, new: PageCount) -> UserArea {
        let old = self.heap_size;
        assert!(new <= old);
        let perm = PTEFlags::U | PTEFlags::R | PTEFlags::W;
        let ubegin = UserAddr4K::heap_offset(new);
        let uend = UserAddr4K::heap_offset(old);
        let area = UserArea::new(ubegin..uend, perm);
        self.heap_size = new;
        area
    }
    // do this when lazy allocation occurs
    pub fn add_alloc_count(&mut self, n: PageCount) {
        self.heap_alloc += n;
    }
    // do this when resize small
    pub fn add_free_count(&mut self, n: PageCount) {
        self.heap_free += n;
    }
}
