use crate::{config, sbi, task::{TaskControlBlock, manager::add_task}, sync::UPSafeCell};
use alloc::{sync::Arc, collections::BinaryHeap};
use riscv::register::time;
use lazy_static::*;

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

/// TimerCondVar 是一个保存 task 和到期时间的结构体。
/// 它们被保存在一个堆中（参见 TIMES），与之相关的方法是：
/// - add_timer
/// - remove_timer
/// - check_timer
pub struct TimerCondVar {
    // task 的到期时间（是一个绝对时间，比如 1000ms 到期就比 900ms 到期的小）
    pub expire_ms: usize,
    pub task: Arc<TaskControlBlock>,
}

impl PartialEq for TimerCondVar {
    fn eq(&self, other: &Self) -> bool {
        self.expire_ms == other.expire_ms
    }
}

impl Eq for TimerCondVar {}

impl PartialOrd for TimerCondVar {
    /// 反向排序，约小的排名越靠前
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        let a = -(self.expire_ms as isize);
        let b = -(other.expire_ms as isize);
        Some(a.cmp(&b))
    }
}

impl Ord for TimerCondVar {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.partial_cmp(other).unwrap()
    }
}

lazy_static! {
    static ref TIMERS: UPSafeCell<BinaryHeap<TimerCondVar>> = unsafe {
        UPSafeCell::new(BinaryHeap::new())
    };
}

/// 向堆（TIMERS）中插入一个 TimeCondVar 结构体
pub fn add_timer(expire_ms: usize, task: Arc<TaskControlBlock>) {
    let mut timers = TIMERS.exclusive_access();
    timers.push(TimerCondVar {
        expire_ms,
        task,
    });
}

/// 从堆（TIMERS）中移除一个 TimeCondVar 结构体
pub fn remove_timer(task: Arc<TaskControlBlock>) {
    let mut timers = TIMERS.exclusive_access();
    let mut temp = BinaryHeap::<TimerCondVar>::new();
    for timer in timers.drain() {
        if Arc::as_ptr(&task) != Arc::as_ptr(&timer.task) {
            temp.push(timer);
        }
    }
    timers.clear();
    timers.append(&mut temp);
}

/// 从堆中不断 peek，将已经过期的 task 添加到调度队列中
pub fn check_timer() {
    let current_ms = get_time_ms();
    let mut timers = TIMERS.exclusive_access();
    while let Some(timer) = timers.peek() {
        if timer.expire_ms <= current_ms {
            add_task(timer.task.clone());
            timers.pop();
        } else {
            break;
        }
    }
}
