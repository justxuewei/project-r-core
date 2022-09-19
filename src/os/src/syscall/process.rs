use alloc::{string::String, sync::Arc, vec::Vec};

use crate::{
    fs::{inode::OpenFlags, open_file},
    mm::page_table::{self, translated_ref, translated_ref_mut},
    task::{
        self,
        manager::{self, get_task_by_pid},
        processor::{self, current_task, current_user_token},
        SignalAction, SignalFlags, MAX_SIG,
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
        processor::current_task()
            .unwrap()
            .exec(data.as_slice(), args_vec);
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

// 注册一个新的 signal action，返回原有的 signal action。
pub fn sys_sigaction(
    signum: i32,
    action: *const SignalAction,
    old_action: *mut SignalAction,
) -> isize {
    let token = current_user_token();
    if current_task().is_none() {
        return -1;
    }
    let task = current_task().unwrap();
    let mut task_inner = task.inner_exclusive_access();
    if signum as usize > MAX_SIG {
        return -1;
    }
    let flag = SignalFlags::from_bits(1 << signum);
    if flag.is_none() {
        return -1;
    }
    let flag = flag.unwrap();
    if action as usize == 0
        || old_action as usize == 0
        || flag == SignalFlags::SIGKILL
        || flag == SignalFlags::SIGSTOP
    {
        return -1;
    }
    let old_action_from_kernel = task_inner.signal_actions.table[signum as usize];
    *translated_ref_mut(token, old_action) = old_action_from_kernel;
    task_inner.signal_actions.table[signum as usize] = *translated_ref(token, action);
    0
}

/// 发送信号
// QUESTION(justxuewei): 为什么发送信号要叫 `sys_kill` 呢？
pub fn sys_kill(pid: usize, signum: i32) -> isize {
    let task = get_task_by_pid(pid);
    if task.is_none() {
        return -1;
    }
    let flag = SignalFlags::from_bits(1 << signum);
    if flag.is_none() {
        return -1;
    }
    let task = task.unwrap();
    let flag = flag.unwrap();
    let mut task_inner = task.inner_exclusive_access();
    if task_inner.signals.contains(flag) {
        return -1;
    }
    task_inner.signals.insert(flag);
    0
}

/// 信号处理结束，返回执行用户逻辑
pub fn sys_sigreturn() -> isize {
    if let Some(task) = current_task() {
        let mut task_inner = task.inner_exclusive_access();
        task_inner.handling_sig = -1;
        let trap_ctx = task_inner.get_trap_cx();
        *trap_ctx = task_inner.trap_ctx_backup.unwrap();
        return 0;
    }
    -1
}

/// 设置进程的信号掩码
pub fn sys_procmask(mask: u32) -> isize {
    if let Some(task) = current_task() {
        let mut task_inner = task.inner_exclusive_access();
        let old_mask = task_inner.signal_mask;
        if let Some(flag) = SignalFlags::from_bits(mask) {
            task_inner.signal_mask = flag;
            return old_mask.bits() as isize;
        }
    }
    -1
}
