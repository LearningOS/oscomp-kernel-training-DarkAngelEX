use crate::{process::Dead, syscall::SysError};

use super::error::OOM;
/// Result<T, `TryRunFail<A>`>
pub type TryR<T, A> = Result<T, TryRunFail<A>>;
/// Async: 这个函数需要被异步版本的函数再次调用一遍.
///
/// Error: 操作失败不需要干掉进程
pub enum TryRunFail<A> {
    Async(A),
    Error(SysError),
}
impl<A> From<Dead> for TryRunFail<A> {
    fn from(_: Dead) -> Self {
        Self::Error(SysError::ESRCH)
    }
}
impl<A, T: OOM> From<T> for TryRunFail<A> {
    fn from(_: T) -> Self {
        Self::Error(SysError::ENOMEM)
    }
}
impl<A> From<SysError> for TryRunFail<A> {
    fn from(e: SysError) -> Self {
        Self::Error(e)
    }
}

/// 用来确保异步调用的正确性
///
/// 每次向空间加入新映射可能会覆盖就映射，这会改变ID防止新映射被就映射修改
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct HandlerID(pub usize);
from_usize_impl!(HandlerID);
impl HandlerID {
    pub fn invalid() -> Self {
        HandlerID(usize::MAX)
    }
}
