use crate::memory::user_ptr::UserInOutPtr;

mod queue;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RobustList {
    pub next: UserInOutPtr<RobustList>,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RobustListHead {
    pub list: RobustList,
    pub futex_offset: usize,
    pub list_op_pending: UserInOutPtr<RobustList>,
}
