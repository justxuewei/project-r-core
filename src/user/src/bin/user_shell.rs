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
use user_lib::{close, dup, exec, fork, open, pipe, waitpid, OpenFlags};

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
        let mut args_copy: Vec<String> = args
            .iter()
            .filter(|arg| !arg.is_empty())
            .map(|arg| {
                let mut arg_copy = String::from(*arg);
                arg_copy.push('\0');
                arg_copy
            })
            .collect();

        let mut input = String::new();
        if let Some((idx, _)) = args_copy
            .iter()
            .enumerate()
            .find(|(_, arg)| arg.as_str() == "<\0")
        {
            input = args_copy[idx + 1].clone();
            args_copy.drain(idx..=idx + 1);
        }

        let mut output = String::new();
        if let Some((idx, _)) = args_copy
            .iter()
            .enumerate()
            .find(|(_, arg)| arg.as_str() == ">\0")
        {
            output = args_copy[idx + 1].clone();
            args_copy.drain(idx..=idx + 1);
        }

        let mut args_addr: Vec<*const u8> = args_copy.iter().map(|arg| arg.as_ptr()).collect();
        args_addr.push(core::ptr::null::<u8>());
        Self {
            input,
            output,
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
                    let split: Vec<_> = line.as_str().split('|').collect();
                    let commands: Vec<_> = split
                        .iter()
                        .map(|cmd| ProcessArguments::new(*cmd))
                        .collect();
                    let mut valid = true;
                    for i in 0..commands.len() {
                        if i == 0 {
                            if !commands[i].output.is_empty() {
                                valid = false;
                            }
                        } else if i == commands.len() - 1 {
                            if !commands[i].input.is_empty() {
                                valid = false;
                            }
                        } else if !commands[i].input.is_empty() || !commands[i].output.is_empty() {
                            valid = false;
                        }
                    }
                    if commands.len() == 1 {
                        valid = true;
                    }
                    if !valid {
                        println!("[user_shell] Invalid command: inputs/outputs cannot be bound correctly.");
                    } else {
                        // 为子进程创建 pipes
                        let mut pipe_fd_list: Vec<[usize; 2]> = Vec::new();
                        if commands.len() > 1 {
                            for _ in 0..commands.len() - 1 {
                                let mut pipe_fd = [0usize; 2];
                                pipe(&mut pipe_fd);
                                pipe_fd_list.push(pipe_fd);
                            }
                        }

                        // 使用 fork 启动子进程
                        let mut children = Vec::new();
                        for (i, cmd) in commands.iter().enumerate() {
                            let pid = fork();
                            if pid == 0 {
                                // === child process ===
                                // redirect input
                                if !cmd.input.is_empty() {
                                    let fd = open(cmd.input.as_str(), OpenFlags::READ_ONLY);
                                    if fd == -1 {
                                        println!("[user_shell] Error when opening file {}", cmd.input);
                                        return -4;
                                    }
                                    let fd = fd as usize;
                                    close(0);
                                    assert!(dup(fd) == 0);
                                    close(fd);
                                }
                                // redirect output
                                if !cmd.output.is_empty() {
                                    let fd = open(
                                        cmd.output.as_str(),
                                        OpenFlags::CREATE | OpenFlags::WRITE_ONLY,
                                    );
                                    if fd == -1 {
                                        println!("[user_shell] Error when opening file {}", cmd.output);
                                        return -4;
                                    }
                                    let fd = fd as usize;
                                    close(1);
                                    assert!(dup(fd) == 1);
                                    close(fd);
                                }
                                // receive input from previous process
                                if i > 0 {
                                    close(0);
                                    assert!(dup(pipe_fd_list[i - 1][0]) == 0);
                                }
                                // send output to next process
                                if i < commands.len() - 1 {
                                    close(1);
                                    assert!(dup(pipe_fd_list[i][1]) == 1);
                                }
                                // fork 会把父进程的全部 fds 拷贝的子进程的，我
                                // 们关心的 pipe 已经被保存到 fd = 0 或者 fd = 1
                                // 的位置了，所以可以放心关闭全部 pipe_fd。
                                for pipe_fd in pipe_fd_list.iter() {
                                    close(pipe_fd[0]);
                                    close(pipe_fd[1]);
                                }
                                // exec new process
                                if exec(cmd.args_copy[0].as_str(), cmd.args_addr.as_slice()) == -1 {
                                    println!("[user_shell] Error when executing {}", cmd.args_copy[0]);
                                    return -4;
                                }
                                unreachable!()
                            } else {
                                // === parent process ===
                                children.push(pid);
                            }
                        }

                        // 这里是 user_shell 进程，与子进程一样，这里申请的
                        // pipes 应该已经被需要的子进程加入到自己的 fd_table 的
                        // 0 或者 1 的位置，而 shell 进程将永远也用不到这些 fd，
                        // 因此需要执行释放处理。
                        for pipe_fd in pipe_fd_list.iter() {
                            close(pipe_fd[0]);
                            close(pipe_fd[1]);
                        }

                        let mut exit_code = 0i32;
                        for pid in children.into_iter() {
                            let exit_pid = waitpid(pid as usize, &mut exit_code);
                            assert_eq!(exit_pid, pid);
                        }
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
