use alloc::sync::Arc;
use spin::Mutex;

use crate::{
    bitmap::{Bitmap, BLOCK_BITS},
    block_cache::{block_cache_sync_all, get_block_cache, BLOCK_SIZE},
    block_dev::BlockDevice,
    layout::{DataBlock, DiskInode, DiskInodeType, SuperBlock},
    vfs::Inode,
};

pub struct EasyFileSystem {
    pub block_device: Arc<dyn BlockDevice>,
    pub inode_bitmap: Bitmap,
    pub data_bitmap: Bitmap,
    inode_area_start_block: u32,
    data_area_start_block: u32,
}

impl EasyFileSystem {
    // 在磁盘上创建一个 efs 文件系统
    pub fn create(
        block_device: Arc<dyn BlockDevice>,
        total_blocks: u32,
        inode_bitmap_blocks: u32,
    ) -> Arc<Mutex<Self>> {
        // inode blocks
        let inode_bitmap = Bitmap::new(1, inode_bitmap_blocks as usize);
        let inode_num = inode_bitmap.maximum(); // inode 的最大数量
        let inode_area_blocks =
            ((inode_num * core::mem::size_of::<DiskInode>() + BLOCK_SIZE - 1) / BLOCK_SIZE) as u32; // inode 占用最大的 blocks 数量
        let inode_total_block = inode_bitmap_blocks + inode_area_blocks;
        // data blocks
        // block 0 是存储 super block 的，所以总块数量是需要 -1 的
        let data_total_blocks = total_blocks - 1 - inode_total_block;
        // 为什么是 BLOCK_BITS + 1 呢？因为整体的存储成本是一个 bitmap 的 block
        // 可以管理 4096 个 blocks，`data_total_blocks + BLOCK_BITS as u32` 则是
        // 表示向上取整 `data_total_blocks + (BLOCK_BITS as u32 + 1) - 1`。
        let data_bitmap_blocks = (data_total_blocks + BLOCK_BITS as u32) / (BLOCK_BITS as u32 + 1);
        let data_area_blocks = data_total_blocks - data_bitmap_blocks;
        let data_bitmap = Bitmap::new(
            (1 + inode_bitmap_blocks + inode_area_blocks) as usize,
            data_bitmap_blocks as usize,
        );
        let mut efs = Self {
            block_device: block_device.clone(),
            inode_bitmap,
            data_bitmap,
            inode_area_start_block: 1 + inode_bitmap_blocks,
            data_area_start_block: 1 + inode_total_block + data_bitmap_blocks,
        };
        // clean all blocks
        for block_id in 0..total_blocks {
            get_block_cache(block_id as usize, block_device.clone())
                .lock()
                .modify(0, |data_block: &mut DataBlock| {
                    for byte in data_block.iter_mut() {
                        *byte = 0;
                    }
                });
        }
        // initialize super block
        get_block_cache(0, block_device.clone()).lock().modify(
            0,
            |super_block: &mut SuperBlock| {
                super_block.initialize(
                    total_blocks,
                    inode_bitmap_blocks,
                    inode_area_blocks,
                    data_bitmap_blocks,
                    data_area_blocks,
                );
            },
        );
        // create the root directory
        assert_eq!(efs.alloc_inode(), 0);
        let (root_inode_block_id, root_inode_offset) = efs.get_disk_inode_pos(0);
        get_block_cache(root_inode_block_id as usize, block_device.clone())
            .lock()
            .modify(0, |disk_inode: &mut DiskInode| {
                disk_inode.initialize(DiskInodeType::Directory);
            });
        // write back immediately
        block_cache_sync_all();

        Arc::new(Mutex::new(efs))
    }

    // 对于一个在磁盘上已经创建的 efs 文件系统，读取 super block 并返回 efs 实例
    pub fn open(block_device: Arc<dyn BlockDevice>) -> Arc<Mutex<Self>> {
        get_block_cache(0, block_device.clone())
            .lock()
            .read(0, |super_block: &SuperBlock| {
                // 根据 magic number 判断是否是 efs
                assert!(super_block.is_valid());
                let inode_total_blocks =
                    super_block.inode_bitmap_blocks + super_block.inode_area_blocks;
                let efs = Self {
                    block_device: block_device.clone(),
                    inode_bitmap: Bitmap::new(1, super_block.inode_bitmap_blocks as usize),
                    data_bitmap: Bitmap::new(
                        (1 + inode_total_blocks) as usize,
                        super_block.data_bitmap_blocks as usize,
                    ),
                    inode_area_start_block: 1 + super_block.inode_bitmap_blocks,
                    data_area_start_block: 1 + inode_total_blocks + super_block.data_bitmap_blocks,
                };
                Arc::new(Mutex::new(efs))
            })
    }

    // 返回 root directory 对应的 inode
    pub fn root_inode(efs: Arc<Mutex<Self>>) -> Inode {
        let block_device = efs.lock().block_device.clone();
        let (root_block_id, root_block_offset) = efs.lock().get_disk_inode_pos(0);
        Inode::new(root_block_id, root_block_offset, efs, block_device)
    }

    // 获取磁盘上 inode id，返回的结果是 block_id 和 block 内的 offset
    fn get_disk_inode_pos(&self, inode_id: u32) -> (u32, usize) {
        let inode_size = core::mem::size_of::<DiskInode>();
        let inodes_pre_block = (BLOCK_SIZE / inode_size) as u32;
        (
            self.inode_area_start_block + (inode_id / inodes_pre_block),
            (inode_id % inodes_pre_block) as usize * inode_size,
        )
    }

    // 将 data block 的 block id 翻译为磁盘上的 block id
    fn get_data_block_id(&self, data_block_id: u32) -> u32 {
        self.data_area_start_block + data_block_id
    }

    // 申请一个 inode，返回 inode 的 id
    fn alloc_inode(&mut self) -> u32 {
        self.inode_bitmap.alloc(self.block_device.clone()).unwrap() as u32
    }

    // 申请一个 data block，返回磁盘 block id（非 data block 的 block id）
    fn alloc_data(&mut self) -> u32 {
        self.get_data_block_id(self.data_bitmap.alloc(self.block_device.clone()).unwrap() as u32)
    }

    // 释放指定的 data block
    fn dealloc_data(&mut self, block_id: u32) {
        // erase data from data area
        get_block_cache(block_id as usize, self.block_device.clone())
            .lock()
            .modify(0, |data_block: &mut DataBlock| {
                for byte in data_block.iter_mut() {
                    *byte = 0;
                }
            });
        // reset data bitmap
        self.data_bitmap.dealloc(
            self.block_device.clone(),
            (block_id - self.data_area_start_block) as usize,
        );
    }
}
