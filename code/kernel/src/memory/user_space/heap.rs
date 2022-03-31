use crate::{
    memory::{
        address::{PageCount, UserAddr4K},
        page_table::PTEFlags,
        user_space::UserArea,
    },
    syscall::{SysError, SysResult},
    tools::range::URange,
};

#[derive(Debug, Clone)]
pub struct HeapManager {
    brk_end: UserAddr4K,
    brk_base: UserAddr4K,
}
impl Drop for HeapManager {
    fn drop(&mut self) {}
}
impl HeapManager {
    pub fn new() -> Self {
        Self {
            brk_end: UserAddr4K::null(),
            brk_base: UserAddr4K::null(),
        }
    }
    pub fn size(&self) -> PageCount {
        self.brk_end.offset_to(self.brk_base)
    }
    pub fn init(&mut self, base: UserAddr4K, init_size: PageCount) -> UserArea {
        self.brk_base = base;
        self.brk_end = base.add_page(init_size);
        UserArea {
            range: self.brk_base..self.brk_end,
            perm: PTEFlags::R | PTEFlags::W | PTEFlags::U,
        }
    }
    pub fn brk_end(&self) -> UserAddr4K {
        self.brk_end
    }
    /// bool: is increase
    pub fn set_brk(
        &mut self,
        brk: UserAddr4K,
        oper: impl FnOnce(UserArea, bool) -> Result<(), SysError>,
    ) -> Result<(), SysError> {
        let cur_end = self.brk_end;
        if brk < self.brk_base {
            return Err(SysError::EINVAL);
        }
        if brk == cur_end {
            return Ok(());
        }
        if brk < cur_end {
            // unmap
            oper(UserArea::new_urw(brk.max(self.brk_base)..cur_end), false)?;
        } else {
            // map
            oper(UserArea::new_urw(cur_end..brk), true)?;
        }
        self.brk_end = brk;
        Ok(())
    }
}
