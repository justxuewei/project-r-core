use alloc::sync::Arc;
use spin::Mutex;

use crate::{
    block_cache::get_block_cache, block_dev::BlockDevice, efs::EasyFileSystem, layout::DiskInode,
};

pub struct Inode {
    block_id: usize,
    block_offset: usize,
    fs: Arc<Mutex<EasyFileSystem>>,
    block_device: Arc<dyn BlockDevice>,
}

impl Inode {
    pub fn new(
        block_id: u32,
        block_offset: usize,
        fs: Arc<Mutex<EasyFileSystem>>,
        block_device: Arc<dyn BlockDevice>,
    ) -> Self {
        Self {
            block_id: block_id as usize,
            block_offset,
            fs,
            block_device,
        }
    }

    // 读取 disk inode
    fn read_disk_inode<V>(&self, f: impl FnOnce(&DiskInode) -> V) -> V {
        get_block_cache(self.block_id, self.block_device.clone())
            .lock()
            .read(self.block_offset, f)
    }

    // 修改 disk inode
    fn modify_disk_inode<V>(&self, f: impl FnOnce(&mut DiskInode) -> V) -> V {
        get_block_cache(self.block_id, self.block_device.clone())
            .lock()
            .modify(self.block_offset, f)
    }
}
