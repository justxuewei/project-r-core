#![no_std]
#![no_main]
#![feature(panic_info_message)]
#![feature(alloc_error_handler)]

// #[macro_use] 的作用是在 mod 作用域结束时依然可以使用 macro，
// 或者引入其他 crate 的 marcos。
// Ref: https://doc.rust-lang.org/reference/macros-by-example.html#the-macro_use-attribute
#[macro_use]
mod console;
mod config;
mod drivers;
pub mod fs;
mod lang_items;
mod loader;
mod mm;
mod sbi;
mod sync;
pub mod syscall;
mod task;
mod timer;
pub mod trap;

#[macro_use]
extern crate alloc;

use core::arch::global_asm;

// load entry.asm：让 RustSBI 知道 rCore 的入口函数是 rust_main
global_asm!(include_str!("entry.asm"));
// 将用户程序链接到操作系统中
global_asm!(include_str!("link_app.S"));

#[no_mangle]
fn rust_main() -> ! {
    clear_bss();

    println!("[kernel] Welcome to rCore!");
    mm::init();
    task::add_initproc();
    trap::init();
    trap::enable_timer_interrupt();
    timer::set_next_trigger();
    task::run_tasks();

    panic!("Unreachable in rust_main")
}

// clear_bss 初始化除了 kernel stack 以外的 .bss 区域
fn clear_bss() {
    extern "C" {
        fn sbss();
        fn ebss();
    }

    (sbss as usize..ebss as usize).for_each(|a| {
        // Performs a volatile write of a memory location with
        // the given value without reading or dropping the old
        // value.
        // Ref: https://doc.rust-lang.org/std/ptr/fn.write_volatile.html
        unsafe { (a as *mut u8).write_volatile(0) }
    })
}
