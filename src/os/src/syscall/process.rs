use alloc::sync::Arc;

use crate::{
    loader,
    mm::page_table,
    task::{self, manager, processor},
    timer,
};

const ANY_PROCESS: isize = -1;

const NO_CHILDREN_RUNNING: isize = -1;
const CHILDREN_RUNNING: isize = -2;

pub fn sys_exit(exit_code: i32) -> ! {
    println!("[kernel] Application exited with code {}", exit_code);
    task::exit_current_and_run_next(exit_code);
    panic!("Unreachable in sys_exit!");
}

pub fn sys_yield() -> isize {
    task::suspend_current_and_run_next();
    0
}

pub fn sys_get_time() -> isize {
    timer::get_time_ms() as isize
}

pub fn sys_getpid() -> isize {
    processor::current_task().unwrap().getpid() as isize
}

pub fn sys_fork() -> isize {
    let parent_tcb = processor::current_task().unwrap();
    let child_tcb = parent_tcb.fork();
    let child_pid = child_tcb.getpid();
    let mut child_trap_cx = child_tcb.inner_exclusive_access().get_trap_cx();
    // child process's return value is 0
    child_trap_cx.x[10] = 0;
    manager::add_task(child_tcb);

    child_pid as isize
}

pub fn sys_exec(path: *const u8) -> isize {
    let token = processor::current_user_token();
    let path = page_table::translated_str(token, path);
    if let Some(data) = loader::get_app_data_by_name(path.as_str()) {
        processor::current_task().unwrap().exec(data);
        return 0;
    }
    -1
}

// 返回数据有三种类型：
// 1. 当关心的子进程处于 Zombie 状态时，返回该进程的 pid (pid >= 0)；
// 2. 当关心的子进程都已经退出时，返回 NO_CHILDREN_RUNNING；
// 3. 当关心的子进程还没有退出时，返回 CHILDREN_RUNNING。
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    let current_task = processor::current_task().unwrap();
    let mut current_task_inner = current_task.inner_exclusive_access();

    if current_task_inner
        .children
        .iter()
        .find(|child| pid == ANY_PROCESS || (pid as usize) == child.getpid())
        .is_none()
    {
        return NO_CHILDREN_RUNNING;
    }

    let pair = current_task_inner
        .children
        .iter()
        .enumerate()
        .find(|(_, child)| {
            child.inner_exclusive_access().is_zombie()
                && (pid == ANY_PROCESS || (pid as usize) == child.getpid())
        });
    if let Some((idx, _)) = pair {
        let child = current_task_inner.children.remove(idx);
        // 确保子进程的强引用在 child 被释放时资源也可以被释放
        assert_eq!(Arc::strong_count(&child), 1);
        let child_pid = child.getpid();
        let exit_code = child.inner_exclusive_access().exit_code;
        *(page_table::translated_ref_mut(current_task_inner.get_user_token(), exit_code_ptr)) =
            exit_code;
        return child_pid as isize;
    }

    CHILDREN_RUNNING
}
