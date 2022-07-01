use ftl_util::error::SysError;

use crate::config::USER_STACK_SIZE;

use super::Process;

const RLIM_INFINITY: u32 = i32::MAX as u32;
const _STK_LIM: u32 = 8 * 1024 * 1024;

const RLIMIT_CPU: u32 = 0;
const RLIMIT_FSIZE: u32 = 1;
const RLIMIT_DATA: u32 = 2;
const RLIMIT_STACK: u32 = 3;
const RLIMIT_CORE: u32 = 4;
const RLIMIT_RSS: u32 = 5;
const RLIMIT_NPROC: u32 = 6;
const RLIMIT_NOFILE: u32 = 7;
const RLIMIT_MEMLOCK: u32 = 8;
const RLIMIT_AS: u32 = 9;
const RLIMIT_LOCKS: u32 = 10;
const RLIMIT_SIGPENDING: u32 = 11;
const RLIMIT_MSGQUEUE: u32 = 12;
const RLIMIT_NICE: u32 = 13;
const RLIMIT_RTPRIO: u32 = 14;
const RLIMIT_RTTIME: u32 = 15;
const RLIM_NLIMITS: u32 = 16;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RLimit {
    rlim_cur: usize, /* Soft limit */
    rlim_max: usize, /* Hard limit (ceiling for rlim_cur) */
}

impl RLimit {
    pub fn new(cur: u32, max: u32) -> Self {
        Self {
            rlim_cur: cur as usize,
            rlim_max: max as usize,
        }
    }
}

pub fn prlimit_impl(
    _proc: &Process,
    resource: u32,
    new: Option<RLimit>,
) -> Result<RLimit, SysError> {
    match resource {
        RLIMIT_CPU => todo!(),
        RLIMIT_FSIZE => todo!(),
        RLIMIT_DATA => todo!(),
        RLIMIT_STACK => {
            // debug_assert!(new.is_none());
            Ok(RLimit::new(USER_STACK_SIZE as u32, RLIM_INFINITY))
        }
        RLIMIT_CORE => todo!(),
        RLIMIT_RSS => todo!(),
        RLIMIT_NPROC => todo!(),
        RLIMIT_NOFILE => todo!(),
        RLIMIT_MEMLOCK => todo!(),
        RLIMIT_AS => todo!(),
        RLIMIT_LOCKS => todo!(),
        RLIMIT_SIGPENDING => todo!(),
        RLIMIT_MSGQUEUE => todo!(),
        RLIMIT_NICE => todo!(),
        RLIMIT_RTPRIO => todo!(),
        RLIMIT_RTTIME => todo!(),
        RLIM_NLIMITS => todo!(),
        _ => Err(SysError::EINVAL),
    }
}
