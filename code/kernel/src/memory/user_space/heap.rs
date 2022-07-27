use ftl_util::error::SysR;

use crate::{
    memory::{
        address::{PageCount, UserAddr, UserAddr4K},
        page_table::PTEFlags,
        user_space::UserArea,
    },
    syscall::SysError,
};

#[derive(Debug, Clone)]
pub struct HeapManager {
    brk: UserAddr<u8>,
    brk_end: UserAddr4K,
    brk_base: UserAddr4K,
}
impl Drop for HeapManager {
    fn drop(&mut self) {}
}
impl HeapManager {
    pub fn new() -> Self {
        Self {
            brk: UserAddr::null(),
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
        self.brk = self.brk_end.into();
        UserArea {
            range: self.brk_base..self.brk_end,
            perm: PTEFlags::R | PTEFlags::W | PTEFlags::U,
        }
    }
    pub fn brk(&self) -> UserAddr<u8> {
        self.brk
    }
    /// bool: unmap
    pub fn set_brk(
        &mut self,
        brk: UserAddr<u8>,
        oper: impl FnOnce(UserArea, bool) -> SysR<()>,
    ) -> SysR<bool> {
        let brk_end_next = brk.ceil();
        let cur_end = self.brk_end;
        if brk_end_next < self.brk_base {
            return Err(SysError::EINVAL);
        }
        let mut unmap = false;
        if brk_end_next == cur_end {
        } else if brk_end_next < cur_end {
            // unmap
            oper(
                UserArea::new_urw(brk_end_next.max(self.brk_base)..cur_end),
                false,
            )?;
            unmap = true;
        } else {
            // map
            oper(UserArea::new_urw(cur_end..brk_end_next), true)?;
        }
        self.brk = brk;
        self.brk_end = brk_end_next;
        Ok(unmap)
    }
}
