mod action;
mod context;
pub mod manager;
mod pid;
pub mod processor;
mod signal;
mod switch;
mod task;

use alloc::sync::Arc;
use lazy_static::*;

use crate::{
    fs::{inode::OpenFlags, open_file},
    task::task::TaskControlBlock,
};

pub use action::{SignalAction, SignalActions};
pub use signal::{handle_signals, SignalFlags, MAX_SIG};
pub use {context::TaskContext, processor::run_tasks};

use self::{manager::remove_from_pid_to_task, processor::current_task, task::TaskStatus};

const INITPROC_NAME: &str = "initproc";

lazy_static! {
    pub static ref INITPROC: Arc<TaskControlBlock> = {
        let initproc_data = open_file(INITPROC_NAME, OpenFlags::READ_ONLY)
            .unwrap()
            .read_all();
        Arc::new(TaskControlBlock::new(initproc_data.as_slice()))
    };
}

pub fn add_initproc() {
    manager::add_task(INITPROC.clone());
}

// 暂停当前任务并切换为 idle 控制流
pub fn suspend_current_and_run_next() {
    let current_task = processor::take_current_task().unwrap();
    let mut current_task_inner = current_task.inner_exclusive_access();
    current_task_inner.task_status = TaskStatus::Ready;
    let current_task_cx_ptr = &mut current_task_inner.task_cx as *mut TaskContext;
    drop(current_task_inner);

    manager::add_task(current_task);
    processor::schedule(current_task_cx_ptr);
}

pub fn exit_current_and_run_next(exit_code: i32) {
    let current_task = processor::take_current_task().unwrap();
    let mut current_task_inner = current_task.inner_exclusive_access();
    current_task_inner.task_status = TaskStatus::Zombie;
    current_task_inner.exit_code = exit_code;
    let mut initproc_inner = INITPROC.inner_exclusive_access();
    for child in current_task_inner.children.iter() {
        child.inner_exclusive_access().parent = Some(Arc::downgrade(&INITPROC));
        initproc_inner.children.push(child.clone());
    }
    current_task_inner.children.clear();
    current_task_inner.memory_set.release_areas();

    remove_from_pid_to_task(current_task.getpid());

    drop(initproc_inner);
    drop(current_task_inner);
    drop(current_task);

    // 这里我有个疑问：`_unused` 何时被释放？
    // `processor::schedule` 这个方法直接调用 `__switch` 方法，
    // `exit_current_and_run_next` 的 `drop` 方法将不会被调用，
    // 但是我们会在 parent 方法的 waitpid 系统调用中清理栈内资源，
    // 同时也要注意的是堆资源是必须手动清理的，比如上面的 `initproc_inner`。
    let mut _unused = TaskContext::zero_init();
    processor::schedule((&mut _unused) as *mut TaskContext)
}

/// 给当前进程添加一个信号
pub fn current_add_signal(flag: SignalFlags) {
    let task = current_task().unwrap();
    let mut task_inner = task.inner_exclusive_access();
    task_inner.signals.insert(flag);
}

/// 返回特殊信号的 ID 和错误信息
pub fn check_signals_error_of_current() -> Option<(i32, &'static str)> {
    let task = current_task().unwrap();
    let task_inner = task.inner_exclusive_access();
    task_inner.signals.check_error()
}
