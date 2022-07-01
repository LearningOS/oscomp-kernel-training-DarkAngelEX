use ftl_util::error::SysError;

use crate::{
    futex::RobustListHead,
    memory::user_ptr::{UserInOutPtr, UserWritePtr},
    process::{search, Tid},
    user::check::UserCheck,
    xdebug::{PRINT_SYSCALL, PRINT_SYSCALL_ALL},
};

use super::{SysResult, Syscall};

const PRINT_SYSCALL_FUTEX: bool = true && PRINT_SYSCALL || PRINT_SYSCALL_ALL;

impl Syscall<'_> {
    pub async fn set_robust_list(&mut self) -> SysResult {
        stack_trace!();
        let (head, len): (UserInOutPtr<RobustListHead>, usize) = self.cx.into();
        if PRINT_SYSCALL_FUTEX {
            println!(
                "set_robust_list head_ptr: {:#x} len_ptr: {}",
                head.as_usize(),
                len
            );
        }
        debug_assert_eq!(len, core::mem::size_of::<RobustListHead>());
        if len != core::mem::size_of::<RobustListHead>() {
            return Err(SysError::EINVAL);
        }
        self.thread.inner().robust_list = head;
        Ok(0)
    }
    pub async fn get_robust_list(&mut self) -> SysResult {
        stack_trace!();
        let (tid, head_ptr, len_ptr): (
            Tid,
            UserWritePtr<UserInOutPtr<RobustListHead>>,
            UserWritePtr<usize>,
        ) = self.cx.into();
        if PRINT_SYSCALL_FUTEX {
            println!(
                "get_robust_list tid: {:?} head_ptr: {:#x} len_ptr: {:#x}",
                tid,
                head_ptr.as_usize(),
                len_ptr.as_usize()
            );
        }
        let head = match tid {
            Tid(0) => self.thread.inner().robust_list,
            tid => {
                search::find_thread(tid)
                    .ok_or(SysError::ESRCH)?
                    .inner()
                    .robust_list
            }
        };
        let uc = UserCheck::new(self.process);
        if !head_ptr.is_null() {
            uc.translated_user_writable_value(head_ptr)
                .await?
                .store(head);
        }
        if !len_ptr.is_null() {
            uc.translated_user_writable_value(len_ptr)
                .await?
                .store(core::mem::size_of::<RobustListHead>());
        }
        Ok(0)
    }
}
