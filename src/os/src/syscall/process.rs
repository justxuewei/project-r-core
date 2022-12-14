use alloc::{string::String, sync::Arc, vec::Vec};

use crate::{
    fs::{inode::OpenFlags, open_file},
    mm::page_table,
    task::{
        self,
        manager::{add_task, get_pcb_by_pid},
        processor::{self, current_process},
        SignalFlags,
    },
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
    processor::current_task()
        .unwrap()
        .process
        .upgrade()
        .unwrap()
        .getpid() as isize
}

pub fn sys_fork() -> isize {
    let parent_tcb = current_process();
    let child_tcb = parent_tcb.fork();
    let child_pid = child_tcb.getpid();
    let child_main_thread = child_tcb.inner_exclusive_access().get_task(0);
    let mut child_trap_cx = child_main_thread.inner_exclusive_access().get_trap_cx();
    // child process's return value is 0
    child_trap_cx.x[10] = 0;
    add_task(child_main_thread);

    child_pid as isize
}

/// exec syscall，
/// path 表示用户程序的地址（目前只能是名字），
/// args 表示用户程序的参数，类型是 [&str]，数据为 0 表明没有更多的参数了
pub fn sys_exec(path: *const u8, mut args: *const usize) -> isize {
    let token = processor::current_user_token();
    let app_name = page_table::translated_str(token, path);
    // args
    let mut args_vec: Vec<String> = Vec::new();
    loop {
        let arg_str_ptr = *page_table::translated_ref(token, args);
        if arg_str_ptr == 0 {
            break;
        }
        args_vec.push(page_table::translated_str(token, arg_str_ptr as *const u8));
        unsafe { args = args.add(1) }
    }
    if let Some(inode) = open_file(app_name.as_str(), OpenFlags::READ_ONLY) {
        let data = inode.read_all();
        let argc = args_vec.len();
        current_process().exec(data.as_slice(), args_vec);
        return argc as isize;
    } else {
        println!(
            "[kernel] Syscall exec error due to opening \"{}\"",
            app_name
        );
    }
    -1
}

// 返回数据有三种类型：
// 1. 当关心的子进程处于 Zombie 状态时，返回该进程的 pid (pid >= 0)；
// 2. 当关心的子进程都已经退出时，返回 NO_CHILDREN_RUNNING；
// 3. 当关心的子进程还没有退出时，返回 CHILDREN_RUNNING。
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();

    if process_inner
        .children
        .iter()
        .find(|child| pid == ANY_PROCESS || (pid as usize) == child.getpid())
        .is_none()
    {
        return NO_CHILDREN_RUNNING;
    }

    let pair = process_inner
        .children
        .iter()
        .enumerate()
        .find(|(_, child)| {
            child.inner_exclusive_access().is_zombie
                && (pid == ANY_PROCESS || (pid as usize) == child.getpid())
        });
    if let Some((idx, _)) = pair {
        let child = process_inner.children.remove(idx);
        // 确保子进程的强引用在 child 被释放时资源也可以被释放
        assert_eq!(Arc::strong_count(&child), 1);
        let child_pid = child.getpid();
        let exit_code = child.inner_exclusive_access().exit_code;
        *(page_table::translated_ref_mut(process_inner.get_user_token(), exit_code_ptr)) =
            exit_code;
        return child_pid as isize;
    }

    CHILDREN_RUNNING
}

/// 发送信号
// QUESTION(justxuewei): 为什么发送信号要叫 `sys_kill` 呢？
pub fn sys_kill(pid: usize, signum: i32) -> isize {
    let process = get_pcb_by_pid(pid);
    if process.is_none() {
        return -1;
    }
    let flag = SignalFlags::from_bits(1 << signum);
    if flag.is_none() {
        return -1;
    }
    let task = process.unwrap();
    let flag = flag.unwrap();
    let mut task_inner = task.inner_exclusive_access();
    if task_inner.signals.contains(flag) {
        return -1;
    }
    task_inner.signals.insert(flag);
    0
}
