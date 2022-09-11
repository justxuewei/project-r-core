#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;

use user_lib::{exec, fork, wait, yield_};

#[no_mangle]
fn main() -> i32 {
    println!("[initproc] Started.");
    if fork() == 0 {
        // child process
        println!("[initproc] User shell will be started.");
        exec("user_shell\0", &[core::ptr::null::<u8>()]);
    } else {
        // parent process
        println!("[initproc] Waiting for user shell to exit.");
        loop {
            let mut exit_code: i32 = 0;
            let pid = wait(&mut exit_code);
            if pid == -1 {
                yield_();
                continue;
            }
            println!(
                "[initproc] Released a zombie process, pid={}, exit_code={}",
                pid, exit_code,
            );
        }
    }
    0
}
