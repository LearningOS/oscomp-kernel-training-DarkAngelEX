use crate::{timer::{self, TimeTicks}, trap::context::TrapContext, scheduler::{app, self}};

pub fn sys_sleep(trap_context: &mut TrapContext, args: [usize; 1]) -> isize {
    let ms = args[0];
    let expire_ms = timer::get_time_ticks() + TimeTicks::from_millisecond(ms);
    scheduler::add_timer_later(expire_ms);
    app::block_current_and_run_next(trap_context);
    0
}
