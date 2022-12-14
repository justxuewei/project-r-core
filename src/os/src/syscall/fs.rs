use crate::{
    fs::{inode::OpenFlags, open_file, pipe},
    mm::page_table::{translated_byte_buffer, translated_ref_mut, translated_str, UserBuffer},
    task::processor::{current_process, current_user_token},
};

/// write buf of length `len` to a file with `fd`
pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    if fd >= process_inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = process_inner.fd_table[fd].clone() {
        drop(process_inner);
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
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    if fd >= process_inner.fd_table.len() {
        return -1;
    }
    let file = process_inner.fd_table[fd].clone();
    if file.is_none() {
        return -1;
    }
    let file = file.unwrap();
    drop(process_inner);
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
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let fd = process_inner.alloc_fd();
    process_inner.fd_table[fd] = Some(file.unwrap());
    fd as isize
}

pub fn sys_close(fd: usize) -> isize {
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    if fd >= process_inner.fd_table.len() {
        return -1;
    }
    if process_inner.fd_table[fd].is_none() {
        return -1;
    }
    process_inner.fd_table[fd].take();
    0
}

/// sys_pipe 将会为进程注册两个 fd，一个用于读，一个用于写，这两个 fd 被保存到
/// pipe_fd。
pub fn sys_pipe(pipe: *mut usize) -> isize {
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let token = current_user_token();
    // 创建 pipes 并保存到进程的 fd_table 中
    let (read_p, write_p) = pipe::make_pipe();
    let read_fd = process_inner.alloc_fd();
    process_inner.fd_table[read_fd] = Some(read_p);
    let write_fd = process_inner.alloc_fd();
    process_inner.fd_table[write_fd] = Some(write_p);
    // 将 read_fd 和 write_fd 传递给用户
    *translated_ref_mut(token, pipe) = read_fd;
    *translated_ref_mut(token, unsafe { pipe.add(1) }) = write_fd;
    0
}

/// sys_dup 复制指定 fd 并将其插入到 fd_table 中
pub fn sys_dup(fd: usize) -> isize {
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    if fd >= process_inner.fd_table.len() {
        return -1;
    }
    if process_inner.fd_table[fd].is_none() {
        return -1;
    }
    let new_fd = process_inner.alloc_fd();
    process_inner.fd_table[new_fd] = process_inner.fd_table[fd].clone();
    new_fd as isize
}
