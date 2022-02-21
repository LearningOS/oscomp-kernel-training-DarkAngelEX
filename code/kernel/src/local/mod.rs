use crate::{hart::cpu, user::UserAccessStatus};

static mut HART_LOCAL: [Local; 64] = [Local::new(); 64];

#[derive(Copy, Clone)]
pub struct Local {
    pub user_access_status: UserAccessStatus,
}

impl Local {
    const fn new() -> Self {
        Self {
            user_access_status: UserAccessStatus::Forbid,
        }
    }
}

pub fn current_local() -> &'static mut Local {
    let i = cpu::hart_id();
    unsafe { &mut HART_LOCAL[i] }
}
