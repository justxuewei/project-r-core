use alloc::sync::{Arc, Weak};

use crate::{sync::UPSafeCell, task::suspend_current_and_run_next};

use super::File;

const RING_BUFFER_SIZE: usize = 32;

pub struct Pipe {
    readable: bool,
    writable: bool,
    buffer: Arc<UPSafeCell<PipeRingBuffer>>,
}

impl Pipe {
    /// 创建一个读端 pipe
    pub fn read_end_with_buffer(buffer: Arc<UPSafeCell<PipeRingBuffer>>) -> Self {
        Self {
            readable: true,
            writable: false,
            buffer,
        }
    }

    /// 创建一个写端 pipe
    pub fn write_end_with_buffer(buffer: Arc<UPSafeCell<PipeRingBuffer>>) -> Self {
        Self {
            readable: false,
            writable: true,
            buffer,
        }
    }
}

impl File for Pipe {
    fn read(&self, buf: crate::mm::UserBuffer) -> usize {
        assert!(self.readable);
        let mut buf_iter = buf.into_iter();
        let mut read_size = 0usize;
        loop {
            let mut pipe_buf = self.buffer.exclusive_access();
            let loop_read_size = pipe_buf.available_read();
            if loop_read_size == 0 {
                // 写端已经关闭，不可能有新数据了
                if pipe_buf.all_write_ends_closed() {
                    return read_size;
                }
                drop(pipe_buf);
                suspend_current_and_run_next();
                continue;
            }
            // 从 pipe 中读取数据
            for _ in 0..loop_read_size {
                if let Some(byte_ref) = buf_iter.next() {
                    unsafe { *byte_ref = pipe_buf.read_byte() }
                    read_size += 1;
                } else {
                    // 用户 buffer 已满，没法写入任何新数据了
                    return read_size;
                }
            }
        }
    }

    fn write(&self, buf: crate::mm::UserBuffer) -> usize {
        assert!(self.writable);
        let mut buf_iter = buf.into_iter();
        let mut write_size = 0usize;
        loop {
            let mut pipe_buf = self.buffer.exclusive_access();
            let loop_write_size = pipe_buf.available_write();
            if loop_write_size == 0 {
                drop(pipe_buf);
                suspend_current_and_run_next();
                continue;
            }
            for _ in 0..loop_write_size {
                if let Some(byte_ptr) = buf_iter.next() {
                    unsafe { pipe_buf.write_byte(*byte_ptr) };
                    write_size += 1;
                } else {
                    return write_size;
                }
            }
        }
    }

    fn readable(&self) -> bool {
        self.readable
    }

    fn writable(&self) -> bool {
        self.writable
    }
}

pub struct PipeRingBuffer {
    arr: [u8; RING_BUFFER_SIZE],
    head: usize,
    tail: usize,
    status: RingBufferStatus,
    write_end: Option<Weak<Pipe>>,
}

impl PipeRingBuffer {
    pub fn new() -> Self {
        Self {
            arr: [0; RING_BUFFER_SIZE],
            head: 0,
            tail: 0,
            status: RingBufferStatus::Empty,
            write_end: None,
        }
    }

    /// 返回 buffer 第一个字节
    pub fn read_byte(&mut self) -> u8 {
        self.status = RingBufferStatus::Normal;
        let b = self.arr[self.head];
        self.head = (self.head + 1) % RING_BUFFER_SIZE;
        if self.head == self.tail {
            self.status = RingBufferStatus::Empty;
        }
        b
    }

    /// 向 buffer 写入一个字节
    pub fn write_byte(&mut self, b: u8) {
        self.status = RingBufferStatus::Normal;
        self.arr[self.tail] = b;
        self.tail = (self.tail + 1) % RING_BUFFER_SIZE;
        if self.head == self.tail {
            self.status = RingBufferStatus::Full;
        }
    }

    /// 返回 buffer 可读取长度
    pub fn available_read(&self) -> usize {
        if self.status == RingBufferStatus::Empty {
            0
        } else if self.tail > self.head {
            self.tail - self.head
        } else {
            RING_BUFFER_SIZE - self.head + self.tail
        }
    }

    /// 返回 buffer 可写长度
    pub fn available_write(&self) -> usize {
        if self.status == RingBufferStatus::Full {
            0
        } else {
            RING_BUFFER_SIZE - self.available_read()
        }
    }

    /// 返回 write_end 是否被释放的信息
    pub fn all_write_ends_closed(&self) -> bool {
        self.write_end.as_ref().unwrap().upgrade().is_none()
    }

    /// 设置写端 pipe 的弱引用
    fn set_write_end(&mut self, write_end: &Arc<Pipe>) {
        self.write_end = Some(Arc::downgrade(write_end));
    }
}

#[derive(Copy, Clone, PartialEq)]
enum RingBufferStatus {
    Full,
    Empty,
    Normal,
}

/// 创建一个读端 pipe 和写端 pipe，他们共享 buffer
pub fn make_pipe() -> (Arc<Pipe>, Arc<Pipe>) {
    let buf = Arc::new(unsafe { UPSafeCell::new(PipeRingBuffer::new()) });
    let read_end = Arc::new(Pipe::read_end_with_buffer(buf.clone()));
    let write_end = Arc::new(Pipe::write_end_with_buffer(buf.clone()));
    buf.exclusive_access().set_write_end(&write_end);
    (read_end, write_end)
}
