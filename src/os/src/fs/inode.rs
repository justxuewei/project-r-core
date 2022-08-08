use alloc::{sync::Arc, vec::Vec};
use bitflags::*;
use easy_fs::{EasyFileSystem, Inode, BLOCK_SIZE};
use lazy_static::*;

use crate::{drivers::block::BLOCK_DEVICE, mm::UserBuffer, sync::UPSafeCell};

use super::File;

lazy_static! {
    pub static ref ROOT_INODE: Arc<Inode> = {
        let efs = EasyFileSystem::open(BLOCK_DEVICE.clone());
        Arc::new(EasyFileSystem::root_inode(efs.clone()))
    };
}

// 为什么要再做一次 OSInode 封装？
// 主要的原因有两个：
// 1. 操作系统读写 inode 的时候需要有权限控制，比如可读性和可写性等，这需要在
//    OSInode 中进行控制；
// 2. 操作系统在读写文件的时候需要维护一个 offset 表示当前文件已经读到哪里了；
// 3. 操作系统需要确保 inode 的访问是无并发的，即一个文件不能同时被两个进程写。
pub struct OSInode {
    readable: bool,
    writable: bool,
    inner: UPSafeCell<OSInodeInner>,
}

pub struct OSInodeInner {
    offset: usize,
    inode: Arc<Inode>,
}

impl OSInode {
    pub fn new(readable: bool, writable: bool, inode: Arc<Inode>) -> Self {
        Self {
            readable,
            writable,
            inner: unsafe { UPSafeCell::new(OSInodeInner { offset: 0, inode }) },
        }
    }

    // 读取 inode 对应的 data block 中的全部数据
    pub fn read_all(&self) -> Vec<u8> {
        let mut inner = self.inner.exclusive_access();
        let mut buf = [0u8; BLOCK_SIZE];
        let mut v: Vec<u8> = Vec::new();
        loop {
            let len = inner.inode.read_at(inner.offset, &mut buf);
            if len == 0 {
                break;
            }
            inner.offset += len;
            v.extend_from_slice(&buf[..len]);
        }
        v
    }
}

impl File for OSInode {
    fn read(&self, mut buf: UserBuffer) -> usize {
        let mut inner = self.inner.exclusive_access();
        let mut total_read_size = 0usize;
        for slice in buf.buffers.iter_mut() {
            let read_size = inner.inode.read_at(inner.offset, *slice);
            if read_size == 0 {
                break;
            }
            inner.offset += read_size;
            total_read_size += read_size;
        }
        total_read_size
    }

    fn write(&self, buf: UserBuffer) -> usize {
        let mut inner = self.inner.exclusive_access();
        let mut total_write_size = 0usize;
        for slice in buf.buffers.iter() {
            let write_size = inner.inode.write_at(inner.offset, *slice);
            assert_eq!(write_size, slice.len());
            inner.offset += write_size;
            total_write_size += write_size;
        }
        total_write_size
    }

    fn readable(&self) -> bool {
        self.readable
    }

    fn writable(&self) -> bool {
        self.writable
    }
}

pub fn list_apps() {
    println!("/***** List Apps *****");
    for app in ROOT_INODE.ls() {
        println!("{}", app);
    }
    println!("*****/");
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

impl OpenFlags {
    // 检查 OpenFlags 目前的读写状态，第一个返回值表示可读性，第二个表示可写性
    pub fn read_write(&self) -> (bool, bool) {
        if self.is_empty() {
            (true, false)
        } else if self.contains(Self::WRITE_ONLY) {
            (false, true)
        } else {
            (true, true)
        }
    }
}

pub fn open_file(name: &str, flags: OpenFlags) -> Option<Arc<OSInode>> {
    let (readable, writable) = flags.read_write();
    // flags == CREATE
    if flags.contains(OpenFlags::CREATE) {
        // 文件已经存在
        if let Some(inode) = ROOT_INODE.find(name) {
            inode.clear();
            return Some(Arc::new(OSInode::new(readable, writable, inode)));
        }
        // 文件不存在，创建文件
        return ROOT_INODE
            .create(name)
            .map(|inode| Arc::new(OSInode::new(readable, writable, inode)));
    }

    // flags == TRUNCATE
    if flags.contains(OpenFlags::TRUNCATE) {
        return ROOT_INODE.find(name).map(|inode| {
            inode.clear();
            Arc::new(OSInode::new(readable, writable, inode))
        });
    }

    None
}
