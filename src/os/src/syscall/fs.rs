use crate::{
    fs::{inode::OpenFlags, open_file},
    mm::page_table::{translated_byte_buffer, translated_str, UserBuffer},
    task::processor::{current_task, current_user_token},
};

/// write buf of length `len` to a file with `fd`
pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    let task = current_task().unwrap();
    let task_inner = task.inner_exclusive_access();
    if fd >= task_inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = task_inner.fd_table[fd].clone() {
        drop(task_inner);
        if !file.writable() {
            return -1;
        }
        return file.write(UserBuffer::new(translated_byte_buffer(
            current_user_token(),
            buf,
            len,
        ))) as isize;
    }
    -1
}

// sys_read 在目前版本中只能接收一个字符，如果字符是 0 则说明没有
// 新的输入，那么就会让出 CPU，反之如果有则将字符保存在 buf 的第一个
// 位置中。
pub fn sys_read(fd: usize, buf: *const u8, len: usize) -> isize {
    let task = current_task().unwrap();
    let task_inner = task.inner_exclusive_access();
    if fd >= task_inner.fd_table.len() {
        return -1;
    }
    let file = task_inner.fd_table[fd].clone();
    if file.is_none() {
        return -1;
    }
    let file = file.unwrap();
    drop(task_inner);
    if !file.readable() {
        return -1;
    }
    file.read(UserBuffer::new(translated_byte_buffer(
        current_user_token(),
        buf,
        len,
    ))) as isize
}

pub fn sys_open(path: *const u8, flags: u32) -> isize {
    let token = current_user_token();
    let name = translated_str(token, path);
    let file = open_file(name.as_str(), OpenFlags::from_bits(flags).unwrap());
    if file.is_none() {
        return -1;
    }
    let task = current_task().unwrap();
    let mut task_inner = task.inner_exclusive_access();
    let fd = task_inner.alloc_fd();
    task_inner.fd_table[fd] = Some(file.unwrap());
    fd as isize
}

pub fn sys_close(fd: usize) -> isize {
    let task = current_task().unwrap();
    let mut task_inner = task.inner_exclusive_access();
    if fd >= task_inner.fd_table.len() {
        return -1;
    }
    if task_inner.fd_table[fd].is_none() {
        return -1;
    }
    task_inner.fd_table[fd].take();
    0
}
