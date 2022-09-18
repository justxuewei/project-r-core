use bitflags::*;

use super::{processor::current_task, suspend_current_and_run_next};

pub const MAX_SIG: usize = 31;

bitflags! {
    pub struct SignalFlags: u32 {
        const SIGDEF = 1; // Default signal handling
        const SIGHUP = 1 << 1;
        const SIGINT = 1 << 2;
        const SIGQUIT = 1 << 3;
        const SIGILL = 1 << 4; // 指令异常
        const SIGTRAP = 1 << 5;
        const SIGABRT = 1 << 6;
        const SIGBUS = 1 << 7;
        const SIGFPE = 1 << 8;
        const SIGKILL = 1 << 9; // 结束进程
        const SIGUSR1 = 1 << 10;
        const SIGSEGV = 1 << 11; // 内存段异常
        const SIGUSR2 = 1 << 12;
        const SIGPIPE = 1 << 13;
        const SIGALRM = 1 << 14;
        const SIGTERM = 1 << 15;
        const SIGSTKFLT = 1 << 16;
        const SIGCHLD = 1 << 17;
        const SIGCONT = 1 << 18; // 恢复进程
        const SIGSTOP = 1 << 19; // 暂停进程
        const SIGTSTP = 1 << 20;
        const SIGTTIN = 1 << 21;
        const SIGTTOU = 1 << 22;
        const SIGURG = 1 << 23;
        const SIGXCPU = 1 << 24;
        const SIGXFSZ = 1 << 25;
        const SIGVTALRM = 1 << 26;
        const SIGPROF = 1 << 27;
        const SIGWINCH = 1 << 28;
        const SIGIO = 1 << 29;
        const SIGPWR = 1 << 30;
        const SIGSYS = 1 << 31;
    }
}

impl SignalFlags {
    pub fn check_error(&self) -> Option<(i32, &'static str)> {
        if self.contains(Self::SIGINT) {
            Some((-2, "Killed, SIGINT=2"))
        } else if self.contains(Self::SIGILL) {
            Some((-4, "Illegal Instruction, SIGILL=4"))
        } else if self.contains(Self::SIGABRT) {
            Some((-6, "Aborted, SIGABRT=6"))
        } else if self.contains(Self::SIGFPE) {
            Some((-8, "Erroneous Arithmetic Operation, SIGFPE=8"))
        } else if self.contains(Self::SIGKILL) {
            Some((-9, "Killed, SIGKILL=9"))
        } else if self.contains(Self::SIGSEGV) {
            Some((-11, "Segmentation Fault, SIGSEGV=11"))
        } else {
            //println!("[K] signalflags check_error  {:?}", self);
            None
        }
    }
}

/// 执行内核的信号处理函数，理论上支持四个信号：
/// - SIGKILL
/// - SIGSTOP
/// - SIGCONT
/// - SIGDEF: 效果与 SIGKILL 一致
fn call_kernel_signal_handler(signal: SignalFlags) {
    let task = current_task().unwrap();
    let mut task_inner = task.inner_exclusive_access();
    match signal {
        SignalFlags::SIGSTOP => {
            if task_inner.signals.contains(SignalFlags::SIGSTOP) {
                task_inner.frozen = true;
                task_inner.signals ^= SignalFlags::SIGSTOP;
            }
        }
        SignalFlags::SIGCONT => {
            if task_inner.signals.contains(SignalFlags::SIGCONT) {
                task_inner.frozen = false;
                task_inner.signals ^= SignalFlags::SIGCONT;
            }
        }
        _ => {
            task_inner.killed = true;
        }
    }
}

/// 执行用户的信号处理函数
fn call_user_signal_handler(sig: usize, flag: SignalFlags) {
    let task = current_task().unwrap();
    let mut task_inner = task.inner_exclusive_access();

    let handler = task_inner.signal_actions.table[sig].handler;
    if handler == 0 {
        println!("[kernel] No user handler found for signal {}, do default action: ignore it or kill process", sig);
        return;
    }

    // 设置 task control block inner 与 signal 相关的字段
    task_inner.signal_mask = task_inner.signal_actions.table[sig].mask;
    task_inner.handling_sig = sig as isize;
    task_inner.signals ^= flag;
    let trap_ctx = task_inner.get_trap_cx();
    // ref:
    // https://kaisery.github.io/trpl-zh-cn/ch04-01-what-is-ownership.html#%E5%8F%98%E9%87%8F%E4%B8%8E%E6%95%B0%E6%8D%AE%E4%BA%A4%E4%BA%92%E7%9A%84%E6%96%B9%E5%BC%8F%E4%BA%8C%E5%85%8B%E9%9A%86
    // TrapContext 实现了 `Copy` trait，一个旧的变量在将其**赋值**给其他变量后仍然可用。
    task_inner.trap_ctx_backup = Some(*trap_ctx);

    trap_ctx.sepc = handler;
    trap_ctx.x[10] = sig;
}

pub fn check_pending_signals() {
    for sig in 0..(MAX_SIG + 1) {
        let task = current_task().unwrap();
        let task_inner = task.inner_exclusive_access();
        let flag = SignalFlags::from_bits(1 << sig).unwrap();
        // 当前没有该信号或者该信号被屏蔽
        if !task_inner.signals.contains(flag) || task_inner.signal_mask.contains(flag) {
            continue;
        }
        if task_inner.handling_sig == -1 {
            // ===== 当前没有正在处理的信号 =====
            drop(task_inner);
            drop(task);
            if flag == SignalFlags::SIGKILL
                || flag == SignalFlags::SIGSTOP
                || flag == SignalFlags::SIGCONT
                || flag == SignalFlags::SIGDEF
            {
                call_kernel_signal_handler(flag);
            } else {
                call_user_signal_handler(sig, flag);
                // 为什么用户的处理程序有个 return 呢？
                // 我考虑了下因为用户的处理程序需要陷入用户的程序执行，只有立即
                // return 才能顺利执行 trap_handler 函数。
                return;
            }
        } else {
            // ===== 当前有正在处理的信号 =====
            // 检查当前信号是否被正在执行的信号屏蔽
            if !task_inner.signal_actions.table[task_inner.handling_sig as usize]
                .mask
                .contains(flag)
            {
                drop(task_inner);
                drop(task);
                if flag == SignalFlags::SIGKILL
                    || flag == SignalFlags::SIGSTOP
                    || flag == SignalFlags::SIGCONT
                    || flag == SignalFlags::SIGDEF
                {
                    call_kernel_signal_handler(flag);
                } else {
                    call_user_signal_handler(sig, flag);
                    // return 原因同上
                    return;
                }
            }
        }
    }
}

/// 处理信号，如果进程被暂停则会持续等待
pub fn handle_signals() {
    check_pending_signals();
    loop {
        let task = current_task().unwrap();
        let task_inner = task.inner_exclusive_access();
        let frozen_flag = task_inner.frozen;
        let killed_flag = task_inner.killed;
        drop(task_inner);
        drop(task);
        if (!frozen_flag) || killed_flag {
            break;
        }
        check_pending_signals();
        suspend_current_and_run_next();
    }
}
