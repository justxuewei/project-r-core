#![no_std]
#![no_main]
#![allow(clippy::println_empty_string)]

extern crate alloc;

#[macro_use]
extern crate user_lib;

const LF: u8 = 0x0au8;
const CR: u8 = 0x0du8;
const DL: u8 = 0x7fu8;
const BS: u8 = 0x08u8;

use alloc::string::String;
use alloc::vec::Vec;
use user_lib::console::getchar;
use user_lib::{exec, fork, waitpid};

struct ProcessArguments {
    // input 重定向地址
    input: String,
    // output 重定向地址
    output: String,
    args_copy: Vec<String>,
    args_addr: Vec<*const u8>,
}

impl ProcessArguments {
    pub fn new(command: &str) -> Self {
        let args: Vec<&str> = command.split(' ').collect();
        let args_copy: Vec<String> = args
            .iter()
            .filter(|arg| !arg.is_empty())
            .map(|arg| {
                let mut arg_copy = String::from(*arg);
                arg_copy.push('\0');
                arg_copy
            })
            .collect();
        // TODO(justxuewei): 暂时忽略 input/output 重定向问题
        let mut args_addr: Vec<*const u8> = args_copy.iter().map(|arg| arg.as_ptr()).collect();
        args_addr.push(core::ptr::null::<u8>());
        Self {
            input: String::from(""),
            output: String::from(""),
            args_copy,
            args_addr,
        }
    }
}

#[no_mangle]
pub fn main() -> i32 {
    println!("[user_shell] Hello, welcome to the user shell!");
    let mut line: String = String::new();
    print!(">> ");
    loop {
        let c = getchar();
        match c {
            LF | CR => {
                println!("");
                if !line.is_empty() {
                    let cmd = ProcessArguments::new(line.as_str());
                    let pid = fork();
                    if pid == 0 {
                        // child process
                        if exec(&cmd.args_copy[0].as_str(), &cmd.args_addr.as_slice()) == -1 {
                            println!("Error when executing!");
                            return -4;
                        }
                        unreachable!();
                    } else {
                        let mut exit_code: i32 = 0;
                        let exit_pid = waitpid(pid as usize, &mut exit_code);
                        assert_eq!(pid, exit_pid);
                        println!(
                            "[user_shell] Process {} exited with code {}",
                            pid, exit_code
                        );
                    }
                    line.clear();
                }
                print!(">> ");
            }
            BS | DL => {
                if !line.is_empty() {
                    print!("{}", BS as char);
                    print!(" ");
                    print!("{}", BS as char);
                    line.pop();
                }
            }
            _ => {
                print!("{}", c as char);
                line.push(c as char);
            }
        }
    }
}
