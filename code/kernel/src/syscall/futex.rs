use core::{sync::atomic::Ordering, time::Duration};

use ftl_util::{error::SysError, time::TimeSpec};

use crate::{
    futex::{RobustListHead, WaitStatus, WakeStatus, FUTEX_BITSET_MATCH_ANY},
    memory::user_ptr::{UserInOutPtr, UserReadPtr, UserWritePtr},
    process::{search, Tid},
    timer,
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

// static FUTEX_LOCK: SleepMutex<()> = SleepMutex::new(());

impl Syscall<'_> {
    ///
    /// FUTEX_WAKE_OP 需要使用 CAS, 其他操作只需要读取值
    ///
    pub async fn sys_futex(&mut self) -> SysResult {
        stack_trace!();
        let (ua, op, val, timeout, ua2, val3): (
            UserInOutPtr<u32>,
            u32,
            u32,
            UserReadPtr<TimeSpec>,
            UserInOutPtr<u32>,
            u32,
        ) = self.cx.into();

        if PRINT_SYSCALL_FUTEX {
            let ua = ua.as_usize();
            let timeout = timeout.as_usize();
            let ua2 = ua2.as_usize();
            let sepc = self.cx.user_sepc;
            println!(
                "sys_futex op:{} ua:{:#x} val:{} timeout:{:#x} ua2:{:#x} val3:{}",
                op, ua, val, timeout, ua2, val3
            );
            println!("    spec:{:#x} ra:{:#x}", sepc, self.cx.ra());
        }

        let val2 = timeout.as_usize() as u32;
        let match_any = FUTEX_BITSET_MATCH_ANY;
        match op & 0xf {
            FUTEX_WAIT => {
                self.futex_wait(op, ua, val, (timeout, true), match_any)
                    .await
            }
            FUTEX_WAKE => self.futex_wake(op, ua, val, match_any).await,
            FUTEX_FD => {
                println!("futex FUTEX_FD has removed");
                Err(SysError::EINVAL)
            }
            FUTEX_REQUEUE => self.futex_requeue(op, ua, ua2, None, val, val2).await,
            FUTEX_CMP_REQUEUE => self.futex_requeue(op, ua, ua2, Some(val3), val, val2).await,
            FUTEX_WAKE_OP => self.futex_wake_op_impl(op, ua, ua2, val, val2, val3).await,
            FUTEX_WAIT_BITSET => self.futex_wait(op, ua, val, (timeout, false), val3).await,
            FUTEX_WAKE_BITSET => self.futex_wake(op, ua, val, val3).await,
            _ => panic!(),
        }
    }
    /// 如果uaddr中的值和val相同则睡眠并等待FUTEX_WAKE按mask唤醒, 如果不同则操作失败并返回EAGAIN。
    ///
    /// mask 不能为 0
    ///
    /// timeout.1: 使用相对时间
    async fn futex_wait(
        &mut self,
        op: u32,
        ua: UserInOutPtr<u32>,
        val: u32,
        timeout: (UserReadPtr<TimeSpec>, bool),
        mask: u32,
    ) -> SysResult {
        stack_trace!();
        if mask == 0 {
            return Err(SysError::EINVAL);
        }
        let timeout = if !timeout.0.is_null() {
            let ts = UserCheck::new(self.process)
                .readonly_value(timeout.0)
                .await?
                .load();
            let mut ts = ts.as_duration();
            if timeout.1 {
                ts += timer::get_time();
            }
            ts
        } else {
            Duration::MAX
        };
        let addr = ua.as_uptr().unwrap();
        let pid = if (op & FUTEX_PRIVATE_FLAG) != 0 {
            Some(self.process.pid())
        } else {
            None
        };
        loop {
            let access = UserCheck::new(self.process).readonly_value(ua).await?;
            let access = &(&*access.access())[0];
            let futex = self.thread.fetch_futex(addr);
            match futex
                .wait(mask, timeout, pid, move || unsafe {
                    core::ptr::read_volatile(access) != val
                })
                .await
            {
                WaitStatus::Ok => return Ok(0),
                WaitStatus::Fail => return Err(SysError::EAGAIN),
                WaitStatus::Closed => continue,
            }
        }
    }
    /// 按mask唤醒 ua 的futex上至多val个线程, 返回被唤醒的线程的数量
    async fn futex_wake(
        &mut self,
        op: u32,
        ua: UserInOutPtr<u32>,
        max: u32,
        mask: u32,
    ) -> SysResult {
        stack_trace!();
        let _ = UserCheck::new(self.process).readonly_value(ua).await?;
        let pid = if (op & FUTEX_PRIVATE_FLAG) != 0 {
            Some(self.process.pid())
        } else {
            None
        };
        let addr = ua.as_uptr().unwrap();
        loop {
            let futex = self.thread.fetch_futex(addr);
            match futex.wake(mask, max as usize, pid, || false) {
                WakeStatus::Ok(n) => return Ok(n),
                WakeStatus::Closed => continue,
                WakeStatus::Fail => unreachable!(),
            }
        }
    }
    /// 如果 ua 指向的值不为 should 则操作失败.
    ///
    /// 如果成功则唤醒 ua 上至多 max_wake 个线程, 剩下的转移至多 max_requeue 到 ua2 上
    ///
    /// 返回被唤醒的线程数量
    async fn futex_requeue(
        &mut self,
        op: u32,
        ua: UserInOutPtr<u32>,
        ua2: UserInOutPtr<u32>,
        should: Option<u32>,
        max_wake: u32,
        max_requeue: u32,
    ) -> SysResult {
        stack_trace!();
        let uc = UserCheck::new(self.process);
        let addr = ua.as_uptr().unwrap();
        let addr2 = ua2.as_uptr().unwrap();
        let pid = if (op & FUTEX_PRIVATE_FLAG) != 0 {
            Some(self.process.pid())
        } else {
            None
        };
        let (n, q) = loop {
            let access = uc.readonly_value(ua).await?;
            let access = &(&*access.access())[0];
            let futex = self.thread.fetch_futex(addr);
            let (s, q) =
                futex.wake_requeue(max_wake as usize, max_requeue as usize, pid, || unsafe {
                    if let Some(v) = should {
                        core::ptr::read_volatile(access) != v
                    } else {
                        false
                    }
                });
            match s {
                WakeStatus::Ok(n) => break (n, q),
                WakeStatus::Fail => return Err(SysError::EAGAIN),
                WakeStatus::Closed => continue,
            }
        };
        let mut q = match q {
            Some(q) => q,
            None => return Ok(n),
        };
        loop {
            let _ = uc.readonly_value(ua2).await?;
            let futex2 = self.thread.fetch_futex(addr2);
            match futex2.append(&mut q) {
                Ok(()) => break,
                Err(()) => continue,
            }
        }
        Ok(n)
    }
    /// 使用CAS操作保存uaddr2上的值并按val3规定修改，唤醒uaddr上futex的至多val个线程，
    ///
    /// 根据uaddr2上先前值的结果唤醒uaddr2的futex上至多val2个线程。
    ///
    /// 返回被唤醒的线程的数量
    async fn futex_wake_op_impl(
        &mut self,
        fop: u32,
        ua: UserInOutPtr<u32>,
        ua2: UserInOutPtr<u32>,
        max1: u32,
        max2: u32,
        mop: u32,
    ) -> SysResult {
        stack_trace!();
        let uc = UserCheck::new(self.process);
        let addr = ua.as_uptr().unwrap();
        let addr2 = ua2.as_uptr().unwrap();
        let pid = if (fop & FUTEX_PRIVATE_FLAG) != 0 {
            Some(self.process.pid())
        } else {
            None
        };
        let op = (mop >> 28) & 0xf;
        let cmp = (mop >> 24) & 0xf;
        let mut oparg = (mop >> 12) & 0xfff;
        let cmparg = mop & 0xfff;
        if op & 8 != 0 {
            oparg = 1 << oparg;
        }
        let op_fn: fn(u32, u32) -> u32 = match op & 0x7 {
            0 => |a, _b| a,
            1 => |a, b| a + b,
            2 => |a, b| a | b,
            3 => |a, b| a & b,
            4 => |a, b| a ^ b,
            _ => return Err(SysError::EINVAL),
        };
        let cmp_fn: fn(u32, u32) -> bool = match cmp {
            0 => |a, b| a == b,
            1 => |a, b| a != b,
            2 => |a, b| a < b,
            3 => |a, b| a <= b,
            4 => |a, b| a > b,
            5 => |a, b| a >= b,
            _ => return Err(SysError::EINVAL),
        };
        let access = uc.atomic_u32(ua2).await?;
        let access = &(&mut *access.access_mut())[0];
        let mut old = access.load(Ordering::Acquire);
        loop {
            match access.compare_exchange(
                old,
                op_fn(old, oparg),
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(v) => old = v,
            }
        }
        let wake2 = cmp_fn(old, cmparg);
        let n1 = loop {
            let _ = uc.readonly_value(ua).await?;
            let futex = self.thread.fetch_futex(addr);
            match futex.wake(FUTEX_BITSET_MATCH_ANY, max1 as usize, pid, || false) {
                WakeStatus::Ok(n) => break n,
                WakeStatus::Closed => continue,
                WakeStatus::Fail => unreachable!(),
            }
        };
        let mut n2 = 0;
        if wake2 {
            n2 = loop {
                let _ = uc.readonly_value(ua2).await?;
                let futex = self.thread.fetch_futex(addr2);
                match futex.wake(FUTEX_BITSET_MATCH_ANY, max2 as usize, pid, || false) {
                    WakeStatus::Ok(n) => break n,
                    WakeStatus::Closed => continue,
                    WakeStatus::Fail => unreachable!(),
                }
            };
        }
        Ok(n1 + n2)
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
