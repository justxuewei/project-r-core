use crate::{config, sbi};
use riscv::register::time;

const TICKS_PER_SEC: usize = 100;
const MSEC_PER_SEC: usize = 1000;

pub fn get_time() -> usize {
    time::read()
}

pub fn get_time_ms() -> usize {
    time::read() / (config::CLOCK_FREQ / MSEC_PER_SEC)
}

// time interrupt will be fired every 10ms
pub fn set_next_trigger() {
    sbi::set_timer(get_time() + config::CLOCK_FREQ / TICKS_PER_SEC);
}
