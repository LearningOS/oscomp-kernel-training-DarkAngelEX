use ftl_util::error::SysError;

use crate::{
    config::{USER_STACK_BEGIN, USER_STACK_END, USER_STACK_SIZE},
    memory::address::{PageCount, UserAddr4K},
    tools::range::URange,
};

#[derive(Clone)]
pub struct StackSpaceManager {
    init_size: PageCount,
    max_size: PageCount,
}

impl StackSpaceManager {
    pub const fn new(init_size: PageCount) -> Self {
        Self {
            init_size,
            max_size: PageCount::page_floor(USER_STACK_SIZE),
        }
    }
    const STACK_END: UserAddr4K = UserAddr4K::from_usize_check(USER_STACK_END);
    pub fn init_area(&self, stack_reverse: PageCount) -> URange {
        Self::STACK_END.sub_page(self.init_size.max(stack_reverse))..Self::STACK_END
    }
    pub fn max_area(&self) -> URange {
        Self::STACK_END.sub_page(self.max_size).add_one_page()..Self::STACK_END
    }
    pub fn init_sp(&self) -> UserAddr4K {
        Self::STACK_END
    }
    pub fn set_max_size(&mut self, size: usize) -> Result<(), SysError> {
        let max = USER_STACK_END - USER_STACK_BEGIN;
        if size > max {
            return Err(SysError::ENOMEM);
        }
        self.max_size = PageCount::page_floor(size);
        Ok(())
    }
}
