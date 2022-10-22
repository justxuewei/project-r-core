mod context;

use crate::{
    config,
    syscall::syscall,
    task::{
        self, check_signals_error_of_current, exit_current_and_run_next, processor, SignalFlags,
    },
    timer,
};
use core::arch::{asm, global_asm};
use riscv::register::{
    mtvec::TrapMode,
    scause::{self, Exception, Interrupt, Trap},
    sie, stval, stvec,
};

pub use context::TrapContext;

global_asm!(include_str!("trap.S"));

pub fn init() {
    set_kernel_trap_entry();
}

fn set_kernel_trap_entry() {
    unsafe {
        stvec::write(trap_from_kernel as usize, TrapMode::Direct);
    }
}

fn set_user_trap_entry() {
    unsafe {
        stvec::write(config::TRAMPOLINE as usize, TrapMode::Direct);
    }
}

// 启用 timer 中断
pub fn enable_timer_interrupt() {
    unsafe {
        sie::set_stimer();
    }
}

#[no_mangle]
/// 当系统从用户态切换到内核态的时候，trap_handler 最先被执行，可以根据陷入的原
/// 因执行相应的逻辑，比如执行 syscall、处理段错误（segment fault）等等。
pub fn trap_handler() -> ! {
    set_kernel_trap_entry();
    let scause = scause::read(); // trap 原因
    let stval = stval::read(); // trap 附加信息
    match scause.cause() {
        Trap::Exception(Exception::UserEnvCall) => {
            let mut trap_cx = processor::current_trap_cx();
            // sepc 目前指向的是 ecall 指令的地址，但是它应该指向的是下一条指令，
            // 已知 ecall 指令的长度为 4，所以这里需要加 4。
            trap_cx.sepc += 4;
            let result =
                syscall(trap_cx.x[17], [trap_cx.x[10], trap_cx.x[11], trap_cx.x[12]]) as usize;
            // trap_cx 在执行 `exec` 被执行后会被回收，
            // 所以这里需要重新获取一个新的 `trap_cx`。
            trap_cx = processor::current_trap_cx();
            trap_cx.x[10] = result
        }
        Trap::Exception(Exception::StoreFault)
        | Trap::Exception(Exception::StorePageFault)
        | Trap::Exception(Exception::LoadFault)
        | Trap::Exception(Exception::LoadPageFault) => {
            println!("[kernel] PageFault in application, kernel killed it.");
            task::current_add_signal(SignalFlags::SIGSEGV);
        }
        Trap::Exception(Exception::IllegalInstruction) => {
            println!("[kernel] IllegalInstruction in application, kernel killed it.");
            task::current_add_signal(SignalFlags::SIGILL);
        }
        Trap::Interrupt(Interrupt::SupervisorTimer) => {
            timer::set_next_trigger();
            task::suspend_current_and_run_next();
        }
        _ => {
            panic!(
                "Unsupported trap: {:?}, stval = {:#x}!",
                scause.cause(),
                stval
            );
        }
    }

    if let Some((sig, msg)) = check_signals_error_of_current() {
        println!("[kernel] Error from signal: {}, sig = {}", msg, sig);
        exit_current_and_run_next(sig);
    }

    trap_return();
}

#[no_mangle]
/// 用于从内核态切换为用户态，并在用户态调用 __restore 方法
pub fn trap_return() -> ! {
    set_user_trap_entry();
    let trap_cx_ptr = config::TRAP_CONTEXT;
    let user_token = processor::current_user_token();
    extern "C" {
        fn __alltraps();
        fn __restore();
    }
    let restore_va = __restore as usize - __alltraps as usize + config::TRAMPOLINE;

    unsafe {
        asm!(
            "fence.i",
            "jr {restore_va}",
            restore_va = in(reg) restore_va,
            in("a0") trap_cx_ptr,
            in("a1") user_token,
            options(noreturn)
        );
    }
}

#[no_mangle]
pub fn trap_from_kernel() -> ! {
    panic!("a trap from kernel")
}
