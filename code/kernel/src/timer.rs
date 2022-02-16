use crate::{
    config::CLOCK_FREQ,
    riscv::{register::time, sbi::set_timer},
};

const TICKS_PER_SEC: usize = 100;
const MSEC_PER_SEC: usize = 1000;

pub fn get_time() -> usize {
    time::read()
}

pub fn get_time_ms() -> usize {
    time::read() / (CLOCK_FREQ / MSEC_PER_SEC)
}

pub fn set_next_trigger() {
    set_timer(get_time() as u64 + (CLOCK_FREQ / TICKS_PER_SEC) as u64);
}
