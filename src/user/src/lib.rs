#![no_std]
#![feature(linkage)]
#![feature(panic_info_message)]
#![feature(alloc_error_handler)]

extern crate alloc;
use alloc::vec::Vec;

#[macro_use]
pub mod console;
mod lang_items;
mod syscall;

#[global_allocator]
static HEAP: LockedHeap = LockedHeap::empty();

#[alloc_error_handler]
pub fn handle_alloc_error(layout: core::alloc::Layout) -> ! {
    panic!("Heap allocation error, layout = {:?}", layout);
}

const USER_HEAP_SIZE: usize = 16384;
static mut HEAP_SPACE: [u8; USER_HEAP_SIZE] = [0; USER_HEAP_SIZE];

// 进程在首次打开的时候会执行 _start 方法，在该方法中进一步执行了函数的主入口
// （main），同时还兼具从 user_sp 中获取 argc 和 argv 的功能。
#[no_mangle]
#[link_section = ".text.entry"]
pub extern "C" fn _start(argc: usize, argv_base: usize) -> ! {
    unsafe {
        HEAP.lock()
            .init(HEAP_SPACE.as_ptr() as usize, USER_HEAP_SIZE);
    }
    let mut argv = Vec::new();
    for i in 0..argc {
        let str_ptr = unsafe {
            ((argv_base + i * core::mem::size_of::<usize>()) as *const usize).read_volatile()
        };
        let str_len = (0usize..)
            .find(|&i| unsafe { ((str_ptr + i) as *const u8).read_volatile() == 0 })
            .unwrap();
        argv.push(
            core::str::from_utf8(unsafe {
                core::slice::from_raw_parts(str_ptr as *const u8, str_len)
            })
            .unwrap(),
        );
    }
    exit(main(argc, &argv));
    panic!("unreachable after sys_exit!");
}

#[linkage = "weak"]
#[no_mangle]
fn main(_argc: usize, _argv: &[&str]) -> i32 {
    panic!("Cannot find main!");
}

use buddy_system_allocator::LockedHeap;
use syscall::*;

const WAITPID_ANY_PID: isize = -1;

pub const WAITPID_NO_CHILDREN_RUNNING: isize = -1;
pub const WAITPID_CHILDREN_RUNNING: isize = -2;

pub fn write(fd: usize, buf: &[u8]) -> isize {
    sys_write(fd, buf)
}

pub fn exit(exit_code: i32) -> isize {
    sys_exit(exit_code)
}

pub fn yield_() -> isize {
    sys_yield()
}

pub fn get_time() -> isize {
    sys_get_time()
}

pub fn getpid() -> isize {
    sys_getpid()
}

pub fn fork() -> isize {
    sys_fork()
}

pub fn exec(path: &str, args: &[*const u8]) -> isize {
    sys_exec(path, args)
}

// wait for all children to exit
pub fn wait(exit_code: &mut i32) -> isize {
    loop {
        match sys_waitpid(WAITPID_ANY_PID, exit_code as *mut i32) {
            WAITPID_CHILDREN_RUNNING => {
                yield_();
            }
            // -1 or a real pid
            exit_pid => return exit_pid,
        }
    }
}

// wait for a specific child to exit
pub fn waitpid(pid: usize, exit_code: &mut i32) -> isize {
    loop {
        match sys_waitpid(pid as isize, exit_code as *mut _) {
            WAITPID_CHILDREN_RUNNING => {
                yield_();
            }
            // -1 or a real pid
            exit_pid => return exit_pid,
        }
    }
}

pub fn sleep(duration: usize) {
    let start = get_time();
    while get_time() - start < duration as isize {
        yield_();
    }
}

pub fn read(fd: usize, buf: &mut [u8]) -> isize {
    sys_read(fd, buf)
}

pub fn pipe(pipe_fd: &mut [usize]) -> isize {
    sys_pipe(pipe_fd)
}
