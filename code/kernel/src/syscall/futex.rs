use ftl_util::error::SysError;

use crate::{
    futex::{RobustListHead, FUTEX_BITSET_MATCH_ANY},
    memory::user_ptr::{UserInOutPtr, UserReadPtr, UserWritePtr},
    process::{search, Tid},
    timer::TimeSpec,
    user::check::UserCheck,
    xdebug::{PRINT_SYSCALL, PRINT_SYSCALL_ALL},
};

use super::{SysResult, Syscall};

const PRINT_SYSCALL_FUTEX: bool = true && PRINT_SYSCALL || PRINT_SYSCALL_ALL;

const FUTEX_PRIVATE_FLAG: u32 = 0x80;
const FUTEX_CLOCK_REALTIME: u32 = 0x100;
const FUTEX_WAIT: u32 = 0;
const FUTEX_WAKE: u32 = 1;
const FUTEX_FD: u32 = 2;
const FUTEX_REQUEUE: u32 = 3;
const FUTEX_CMP_REQUEUE: u32 = 4;
const FUTEX_WAKE_OP: u32 = 5;
const FUTEX_WAIT_BITSET: u32 = 9;
const FUTEX_WAKE_BITSET: u32 = 10;

impl Syscall<'_> {
    ///
    /// FUTEX_WAKE_OP 需要使用 CAS, 其他操作只需要读取值
    ///
    pub async fn sys_futex(&mut self) -> SysResult {
        let (uaddr, futex_op, val, timeout, uaddr2, val3): (
            UserInOutPtr<u32>,
            u32,
            u32,
            UserReadPtr<TimeSpec>,
            UserInOutPtr<u32>,
            u32,
        ) = self.cx.into();
        let val2 = timeout.as_usize() as u32;
        match futex_op & 0xf {
            FUTEX_WAIT => {
                self.futex_wait_bitset_impl(futex_op, uaddr, val, timeout, FUTEX_BITSET_MATCH_ANY)
                    .await
            }
            FUTEX_WAKE => {
                self.futex_wake_bitset_impl(futex_op, uaddr, val, FUTEX_BITSET_MATCH_ANY)
                    .await
            }
            FUTEX_FD => {
                println!("futex FUTEX_FD has removed");
                Err(SysError::EINVAL)
            }
            FUTEX_REQUEUE => {
                self.futex_cmp_requeue_impl(futex_op, uaddr, uaddr2, Some(val3), val, val2)
                    .await
            }
            FUTEX_CMP_REQUEUE => {
                self.futex_cmp_requeue_impl(futex_op, uaddr, uaddr2, Some(val3), val, val2)
                    .await
            }
            FUTEX_WAKE_OP => {
                self.futex_wake_op_impl(futex_op, uaddr, uaddr2, val, val2, val3)
                    .await
            }
            FUTEX_WAIT_BITSET => {
                self.futex_wait_bitset_impl(futex_op, uaddr, val, timeout, val3)
                    .await
            }
            FUTEX_WAKE_BITSET => {
                self.futex_wake_bitset_impl(futex_op, uaddr, val, val3)
                    .await
            }
            _ => panic!(),
        }
    }
    /// 如果uaddr中的值和val相同则睡眠并等待FUTEX_WAKE按mask唤醒, 如果不同则操作失败并返回EAGAIN。
    ///
    /// mask 不能为 0
    async fn futex_wait_bitset_impl(
        &mut self,
        _futex_op: u32,
        _uaddr: UserInOutPtr<u32>,
        _val: u32,
        _timeout: UserReadPtr<TimeSpec>,
        _mask: u32,
    ) -> SysResult {
        todo!()
    }
    /// 按mask唤醒uaddr的futex上至多val个线程, 返回被唤醒的线程的数量
    async fn futex_wake_bitset_impl(
        &mut self,
        _futex_op: u32,
        _uaddr: UserInOutPtr<u32>,
        _max: u32,
        _mask: u32,
    ) -> SysResult {
        todo!()
    }
    /// 如果 uaddr 指向的值不为 should 则操作失败.
    ///
    /// 如果成功则唤醒uaddr上至多max_wake个线程, 剩下的转移至多max_requeue到max_requeue上
    ///
    /// 返回被唤醒的线程数量
    async fn futex_cmp_requeue_impl(
        &mut self,
        _futex_op: u32,
        _uaddr: UserInOutPtr<u32>,
        _uaddr2: UserInOutPtr<u32>,
        _should: Option<u32>,
        _max_wake: u32,
        _max_requeue: u32,
    ) -> SysResult {
        todo!()
    }
    /// 使用CAS操作保存uaddr2上的值并按val3规定修改，唤醒uaddr上futex的至多val个线程，
    /// 根据uaddr2上先前值的结果唤醒uaddr2的futex上至多val2个线程。
    ///
    /// 返回被唤醒的线程的数量
    async fn futex_wake_op_impl(
        &mut self,
        _futex_op: u32,
        _uaddr: UserInOutPtr<u32>,
        _uaddr2: UserInOutPtr<u32>,
        _max1: u32,
        _max2: u32,
        _op: u32,
    ) -> SysResult {
        todo!()
    }
    pub async fn sys_set_robust_list(&mut self) -> SysResult {
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
    pub async fn sys_get_robust_list(&mut self) -> SysResult {
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
            uc.writable_value(head_ptr).await?.store(head);
        }
        if !len_ptr.is_null() {
            uc.writable_value(len_ptr)
                .await?
                .store(core::mem::size_of::<RobustListHead>());
        }
        Ok(0)
    }
}
