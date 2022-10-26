#![no_std]
#![feature(linkage)]
#![feature(panic_info_message)]
#![feature(alloc_error_handler)]

// ===== extern crate section =====
extern crate alloc;

// ===== mod section =====
#[macro_use]
pub mod console;
mod lang_items;
use bitflags::*;
mod syscall;
pub mod syscall_signal;

// ===== use section =====
use alloc::vec::Vec;
use buddy_system_allocator::LockedHeap;
use syscall::*;
pub use syscall_signal::*;

// ===== static section =====
#[global_allocator]
static HEAP: LockedHeap = LockedHeap::empty();
static mut HEAP_SPACE: [u8; USER_HEAP_SIZE] = [0; USER_HEAP_SIZE];

// ===== const section =====
const USER_HEAP_SIZE: usize = 16384;
const WAITPID_ANY_PID: isize = -1;
pub const WAITPID_NO_CHILDREN_RUNNING: isize = -1;
pub const WAITPID_CHILDREN_RUNNING: isize = -2;

#[alloc_error_handler]
pub fn handle_alloc_error(layout: core::alloc::Layout) -> ! {
    panic!("Heap allocation error, layout = {:?}", layout);
}

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

bitflags! {
    pub struct OpenFlags: u32 {
        const READ_ONLY = 0;
        const WRITE_ONLY = 1 << 0;
        const READ_WRITE = 1 << 1;
        const CREATE = 1 << 9;
        const TRUNCATE = 1 << 10;
    }
}

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

pub fn open(path: &str, flags: OpenFlags) -> isize {
    sys_open(path, flags)
}

pub fn close(fd: usize) -> isize {
    sys_close(fd)
}

pub fn dup(fd: usize) -> isize {
    sys_dup(fd)
}

pub fn kill(pid: usize, signal: i32) -> isize {
    sys_kill(pid, signal)
}

pub fn thread_create(entry: usize, arg: usize) -> isize {
    sys_thread_create(entry, arg)
}

pub fn gettid() -> isize {
    sys_gettid()
}

pub fn waittid(tid: usize) -> isize {
    loop {
        match sys_waittid(tid) {
            -2 => {
                yield_();
            }
            exit_code => return exit_code,
        }
    }
}

pub fn mutex_create() -> isize {
    sys_mutex_create(false)
}

pub fn mutex_blocking_create() -> isize {
    sys_mutex_create(true)
}

pub fn mutex_lock(mutex_id: usize) {
    sys_mutex_lock(mutex_id);
}

pub fn mutex_unlock(mutex_id: usize) {
    sys_mutex_unlock(mutex_id);
}
