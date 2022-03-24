use crate::memory::{
    address::{PageCount, UserAddr4K},
    page_table::PTEFlags,
    user_space::UserArea,
};

#[derive(Debug, Clone)]
pub struct HeapManager {
    heap_size: PageCount,
}
impl Drop for HeapManager {
    fn drop(&mut self) {}
}
impl HeapManager {
    pub fn new() -> Self {
        Self {
            heap_size: PageCount(0),
        }
    }
    pub fn size(&self) -> PageCount {
        self.heap_size
    }
    pub fn set_size_bigger(&mut self, new: PageCount) -> UserArea {
        let old = self.heap_size;
        assert!(new >= old);
        let perm = PTEFlags::U | PTEFlags::R | PTEFlags::W;
        let ubegin = UserAddr4K::heap_offset(old);
        let uend = UserAddr4K::heap_offset(new);
        let area = UserArea::new(ubegin..uend, perm);
        self.heap_size = new;
        area
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
}
