use alloc::sync::Arc;

use crate::{
    mm::memory_set::kernel_token,
    task::{
        manager::add_task,
        processor::{current_process, current_task},
        TaskControlBlock,
    },
    trap::{trap_handler, TrapContext},
};

/// 创建线程
pub fn sys_thread_create(entry: usize, arg: usize) -> isize {
    let task = current_task().unwrap();
    let process = task.process.upgrade().unwrap();
    // 创建一个任务
    let task_inner = task.inner_exclusive_access();
    let task_res = task_inner.res.as_ref().unwrap();
    let new_task = Arc::new(TaskControlBlock::new(
        process.clone(),
        task_res.ustack_base,
        true,
    ));
    // 将 task 添加进 scheduler
    add_task(new_task.clone());
    // 添加 task 到 process
    let mut process_inner = process.inner_exclusive_access();
    while process_inner.tasks.len() <= task_res.tid {
        process_inner.tasks.push(None);
    }
    process_inner.tasks[task_res.tid] = Some(new_task.clone());
    // 初始化 trap context
    let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
    *trap_cx = TrapContext::app_init_context(
        entry,
        task_res.ustack_top(),
        kernel_token(),
        task.kstack.get_top(),
        trap_handler as usize,
    );
    trap_cx.x[10] = arg;

    task_res.tid as isize
}

pub fn sys_waittid(tid: usize) -> isize {
    let task = current_task().unwrap();
    let task_inner = task.inner_exclusive_access();
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();

    // 不能自己等自己结束
    if task_inner.res.as_ref().unwrap().tid == tid {
        println!("[kernel] a thread cannot wait itself");
        return -1;
    }
    let mut exit_code: Option<i32> = None;
    let waited_task = process_inner.tasks[tid].as_ref();
    if let Some(waited_task) = waited_task {
        exit_code = waited_task.inner_exclusive_access().exit_code;
    } else {
        println!("[kernel] tid {} not found", tid);
        return -1;
    }

    if let Some(exit_code) = exit_code {
        process_inner.tasks[tid] = None;
        exit_code as isize
    } else {
        // 等待的线程还没有结束运行
        -2
    }
}

pub fn sys_gettid() -> isize {
    current_task()
        .unwrap()
        .inner_exclusive_access()
        .res
        .as_ref()
        .unwrap()
        .tid as isize
}
