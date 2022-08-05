use crate::mm::UserBuffer;

mod inode;

pub trait File: Send + Sync {
    fn readable(&self) -> bool;
    fn writable(&self) -> bool;
    // read data from fs to buffer
    fn read(&self, buf: UserBuffer) -> usize;
    // write data from buffer to fs
    fn write(&self, buf: UserBuffer) -> usize;
}
