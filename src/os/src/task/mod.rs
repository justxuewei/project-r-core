mod context;
mod id;
pub mod manager;
mod process;
pub mod processor;
mod signal;
mod switch;
mod task;

use alloc::sync::Arc;
use lazy_static::*;

use crate::{
    fs::{inode::OpenFlags, open_file},
    task::process::ProcessControlBlock,
};

pub use signal::SignalFlags;
pub use task::TaskControlBlock;
pub use {context::TaskContext, processor::run_tasks};

use self::{
    manager::{add_task, remove_from_pid_to_pcb},
    processor::{current_process, current_task, schedule, take_current_task},
    task::TaskStatus,
};

const INITPROC_NAME: &str = "initproc";

lazy_static! {
    pub static ref INITPROC: Arc<ProcessControlBlock> = {
        let initproc_data = open_file(INITPROC_NAME, OpenFlags::READ_ONLY)
            .unwrap()
            .read_all();
        ProcessControlBlock::new(initproc_data.as_slice())
    };
}

pub fn add_initproc() {
    let _initproc = INITPROC.clone();
}

// 暂停当前任务并切换为 idle 控制流
pub fn suspend_current_and_run_next() {
    let current_task = take_current_task().unwrap();
    let mut current_task_inner = current_task.inner_exclusive_access();
    current_task_inner.task_status = TaskStatus::Ready;
    let current_task_cx_ptr = &mut current_task_inner.task_cx as *mut TaskContext;
    drop(current_task_inner);

    add_task(current_task);
    schedule(current_task_cx_ptr);
}

/// 退出当前进程并运行下一个进程
pub fn exit_current_and_run_next(exit_code: i32) {
    let task = current_task().unwrap();
    let mut task_inner = task.inner_exclusive_access();
    let process = task.process.upgrade().unwrap();
    let tid = task_inner.res.as_ref().unwrap().tid;

    task_inner.exit_code = Some(exit_code);
    // 释放 tid 和线程资源
    task_inner.res = None;

    drop(task_inner);
    drop(task);

    // 如果主线程（tid == 0）被终止，那么进程也需要被终止
    if tid == 0 {
        remove_from_pid_to_pcb(process.getpid());
        let mut process_inner = process.inner_exclusive_access();
        process_inner.is_zombie = true;
        process_inner.exit_code = exit_code;

        // 将当前进程的子进程挂到 initproc 上
        let mut initproc_inner = INITPROC.inner_exclusive_access();
        for child in process_inner.children.iter() {
            child.inner_exclusive_access().parent = Some(Arc::downgrade(&INITPROC));
            initproc_inner.children.push(Arc::clone(child));
        }
        drop(initproc_inner);

        // 释放当前进程的全部线程的资源（比如 ustack 等）
        for task in process_inner.tasks.iter().filter(|t| t.is_some()) {
            let task = task.as_deref().unwrap();
            let mut task_inner = task.inner_exclusive_access();
            task_inner.res = None;
        }

        process_inner.children.clear();
        process_inner.memory_set.recycle_data_pages();
    }

    drop(process);

    // _unused 依然存储在 kernel stack 中，需要等待 waitpid 的进程对其释放
    let mut _unused = TaskContext::zero_init();
    schedule((&mut _unused) as *mut TaskContext);
}

/// 阻塞当前线程并运行下一个
pub fn block_current_and_run_next() {
    let task = current_task().unwrap();
    let mut task_inner = task.inner_exclusive_access();
    let task_cx_ptr = &mut task_inner.task_cx as *mut TaskContext;
    task_inner.task_status = TaskStatus::Blocking;
    drop(task_inner);
    schedule(task_cx_ptr);
}

/// 给当前进程添加一个信号
pub fn current_add_signal(flag: SignalFlags) {
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    process_inner.signals.insert(flag);
}

/// 返回特殊信号的 ID 和错误信息
pub fn check_signals_error_of_current() -> Option<(i32, &'static str)> {
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    process_inner.signals.check_error()
}
