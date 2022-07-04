use core::sync::atomic::{AtomicU64, Ordering};

use crate::{
    memory::user_ptr::UserWritePtr, timer, tools, user::check::UserCheck, xdebug::CLOSE_RANDOM,
};

use super::{SysResult, Syscall};

bitflags! {
    pub struct GRND: u32 {
        const NONBLOCK = 1 << 0;
        const RANDOM   = 1 << 1;
    }
}

static RANDOM_STATE: AtomicU64 = AtomicU64::new(0);

pub fn fetch_random_state() -> (u64, u64) {
    loop {
        let old = RANDOM_STATE.load(Ordering::Relaxed);
        let seed = match CLOSE_RANDOM {
            false => (timer::get_time_ticks().into_usize() as u64) ^ 0x16785955_81750151,
            true => 1,
        };
        let new = tools::xor_shift_128_plus((seed, old));
        if RANDOM_STATE
            .compare_exchange(old, new.1, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            break new;
        }
    }
}

impl Syscall<'_> {
    pub async fn sys_getrandom(&mut self) -> SysResult {
        stack_trace!();
        let (buf, len, flags): (UserWritePtr<u8>, usize, u32) = self.cx.into();
        let _flags = unsafe { GRND::from_bits_unchecked(flags) };
        let buffer = UserCheck::new(self.process)
            .translated_user_writable_slice(buf, len)
            .await?;
        let mut seed = fetch_random_state();
        for s in buffer.access_mut().chunks_mut(u64::BITS as usize) {
            seed = tools::xor_shift_128_plus(seed);
            let bytes = u64::to_ne_bytes(seed.1);
            s.iter_mut()
                .zip(bytes.iter())
                .for_each(|(dst, src)| *dst = *src);
        }
        Ok(buffer.len())
    }
}
