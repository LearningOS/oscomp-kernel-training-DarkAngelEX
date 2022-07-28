#[macro_use]
pub mod stack;

static mut CURRENT_SIE: Option<fn() -> bool> = None;

pub fn sie_init(current_sie: fn() -> bool) {
    unsafe { CURRENT_SIE.replace(current_sie) };
}

pub fn assert_sie_closed() {
    stack_trace!();
    if let Some(f) = unsafe { CURRENT_SIE } {
        assert!(!f());
    }
}

/// 死锁检测器
pub struct DeadLockCheck<const N: usize = 1000000>(usize);

impl<const N: usize> const Default for DeadLockCheck<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> DeadLockCheck<N> {
    #[inline]
    pub const fn new() -> Self {
        Self(0)
    }
    #[inline]
    #[allow(clippy::result_unit_err)]
    pub fn step(&mut self) -> Result<(), ()> {
        if cfg!(not(debug_assertions)) {
            return Ok(());
        }
        if self.0 >= N {
            return Err(());
        }
        self.0 += 1;
        Ok(())
    }
}
