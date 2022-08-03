use crate::{
    mm::page_table,
    sbi,
    task::{self, processor},
};

const FD_STDIN: usize = 0;
const FD_STDOUT: usize = 1;

/// write buf of length `len` to a file with `fd`
pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    match fd {
        FD_STDOUT => {
            let buffers =
                page_table::translated_byte_buffer(processor::current_user_token(), buf, len);
            for buffer in buffers {
                print!("{}", core::str::from_utf8(buffer).unwrap());
            }
            len as isize
        }
        _ => {
            panic!("Unsupported fd in sys_write!");
        }
    }
}

// sys_read 在目前版本中只能接收一个字符，如果字符是 0 则说明没有
// 新的输入，那么就会让出 CPU，反之如果有则将字符保存在 buf 的第一个
// 位置中。
pub fn sys_read(fd: usize, buf: *const u8, len: usize) -> isize {
    match fd {
        FD_STDIN => {
            assert!(len == 1, "Only support len == 1 in sys_read!");
            let mut c: usize;
            loop {
                c = sbi::console_getchar();
                if c == 0 {
                    task::suspend_current_and_run_next();
                    continue;
                } else {
                    break;
                }
            }

            let ch = c as u8;
            let mut buffers =
                page_table::translated_byte_buffer(processor::current_user_token(), buf, len);
            unsafe {
                buffers[0].as_mut_ptr().write_volatile(ch);
            }
            0
        }
        _ => {
            panic!("Unsupported fd in sys_read!");
        }
    }
}
