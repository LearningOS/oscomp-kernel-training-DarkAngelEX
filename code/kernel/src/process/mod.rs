mod context;
mod pid;
mod switch;

pub use switch::switch;

use crate::{config::PAGE_SIZE, memory::UserPageTable, trap::context::TrapContext};

use self::context::SwitchContext;

#[derive(Copy, Clone, PartialEq)]
pub enum TaskStatus {
    Ready,
    Running,
    Exited,
}

// sync with pagetable.
pub struct UserSpace {
    stack: usize,
    heap: usize,
}

pub struct ProcessControlBlock {
    page_table: UserPageTable,      // auto free space
    task_status: TaskStatus,        // switch in kernel
    trap_context: TrapContext,      // switch between kernel and user
    switch_context: SwitchContext,  //
    user_space: UserSpace,          //
}

impl ProcessControlBlock {
    pub fn new(elf_data: &[u8], app_id: usize) -> Self {
        assert!(core::mem::size_of::<ProcessControlBlock>() < PAGE_SIZE, "size of ProcessControlBlock is too large!");
        // memory_set with elf program headers/trampoline/trap context/user stack
        // let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
        todo!()
    }
}
