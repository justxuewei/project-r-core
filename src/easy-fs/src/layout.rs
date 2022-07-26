use alloc::{sync::Arc, vec::Vec};

use crate::{
    block_cache::{get_block_cache, BLOCK_SIZE},
    block_dev::BlockDevice,
};

const EFS_MAGIC: u32 = 0x3b800001;
const INODE_DIRECT_COUNT: usize = 28;
const INODE_INDIRECT1_COUNT: usize = BLOCK_SIZE / 4;
const INODE_INDIRECT2_COUNT: usize = INODE_INDIRECT1_COUNT * INODE_INDIRECT1_COUNT;
const DIRECT_BOUND: usize = INODE_DIRECT_COUNT;
const INDIRECT1_BOUND: usize = DIRECT_BOUND + INODE_INDIRECT1_COUNT;
const INDIRECT2_BOUND: usize = INDIRECT1_BOUND + INODE_INDIRECT2_COUNT;
const NAME_LENGTH_LIMIT: usize = 27;
const DIR_ENTRY_SIZE: usize = 32;

#[repr(C)]
pub struct SuperBlock {
    magic: u32,
    pub total_blocks: u32,
    pub inode_bitmap_blocks: u32,
    pub inode_area_blocks: u32,
    pub data_bitmap_blocks: u32,
    pub data_area_blocks: u32,
}

impl SuperBlock {
    pub fn initialize(
        &mut self,
        total_blocks: u32,
        inode_bitmap_blocks: u32,
        inode_area_blocks: u32,
        data_bitmap_blocks: u32,
        data_area_blocks: u32,
    ) {
        *self = Self {
            magic: EFS_MAGIC,
            total_blocks,
            inode_bitmap_blocks,
            inode_area_blocks,
            data_bitmap_blocks,
            data_area_blocks,
        };
    }

    pub fn is_valid(&self) -> bool {
        self.magic == EFS_MAGIC
    }
}

// 目前 easyfs 只支持文件和文件夹两种类型的 inode
#[derive(PartialEq)]
pub enum DiskInodeType {
    File,
    Directory,
}

type IndirectBlock = [u32; INODE_INDIRECT1_COUNT];
pub type DataBlock = [u8; BLOCK_SIZE];

// DiskInode 表示一个文件或目录，
// 如果 INODE_DIRECT_COUNT 的长度为 28，则 DiskInode 的长度为 32 * 4B = 128B，所
// 以一个 block 可以存储 4 个 DiskInode
#[repr(C)]
pub struct DiskInode {
    pub size: u32,
    // 如果 inode 的数量小于 INODE_DIRECT_COUNT（28 个），则直接存在 direct 中
    pub direct: [u32; INODE_DIRECT_COUNT],
    // 如果 inode 的数量大于 INODE_DIRECT_COUNT，则存在 indirect1 中，每个
    // indirect1 指向一个 block，可以覆盖 (512B / 4B) * 512B = 64KB 的内容
    pub indirect1: u32,
    // 如果 inode 的数量大于 512 个，则存在 indirect2 中，每个 indirect2 的一个
    // 数据项指向一个 indirect1，可以覆盖 (512B / 4B) * 64KB = 8MB 的内容。
    pub indirect2: u32,
    type_: DiskInodeType,
}

impl DiskInode {
    pub fn initialize(&mut self, type_: DiskInodeType) {
        self.size = 0;
        self.direct = [0; INODE_DIRECT_COUNT];
        self.indirect1 = 0;
        self.indirect2 = 0;
        self.type_ = type_;
    }

    pub fn is_dir(&self) -> bool {
        self.type_ == DiskInodeType::Directory
    }

    pub fn is_file(&self) -> bool {
        self.type_ == DiskInodeType::File
    }

    // 从 inode 中获取 data block 的 block id
    pub fn get_block_id(&self, inner_id: u32, block_device: Arc<dyn BlockDevice>) -> u32 {
        let inner_id = inner_id as usize;
        assert!(inner_id < INDIRECT2_BOUND);
        if inner_id < DIRECT_BOUND {
            self.direct[inner_id]
        } else if inner_id < INDIRECT1_BOUND {
            get_block_cache(self.indirect1 as usize, block_device.clone())
                .lock()
                .read(0, |indirect_block: &IndirectBlock| {
                    indirect_block[inner_id - DIRECT_BOUND]
                })
        } else {
            let inner_id = inner_id - INDIRECT1_BOUND;
            let indirect1_block_id = get_block_cache(self.indirect2 as usize, block_device.clone())
                .lock()
                .read(0, |indirect2_block: &IndirectBlock| {
                    indirect2_block[inner_id / INODE_INDIRECT1_COUNT] as usize
                });
            get_block_cache(indirect1_block_id, block_device.clone())
                .lock()
                .read(0, |indirect1_block: &IndirectBlock| {
                    indirect1_block[inner_id % INODE_INDIRECT1_COUNT]
                })
        }
    }

    fn _data_blocks(size: u32) -> u32 {
        (size + BLOCK_SIZE as u32 - 1) / BLOCK_SIZE as u32
    }

    // 返回当前 inode 占用 data block 的数量
    pub fn data_blocks(&self) -> u32 {
        Self::_data_blocks(self.size)
    }

    // 计算 size 需要的 data block 的数量
    pub fn total_blocks(size: u32) -> u32 {
        let data_blocks = Self::_data_blocks(size) as usize;
        let mut total = data_blocks;
        if data_blocks > DIRECT_BOUND {
            total += 1;
        } else if data_blocks > INDIRECT1_BOUND {
            total += 2;
            total +=
                (data_blocks - INDIRECT1_BOUND + INODE_INDIRECT1_COUNT - 1) / INODE_INDIRECT1_COUNT;
        }
        total as u32
    }

    // 传入一个新的 new_size，返回需要新增 blocks 数量，
    // 需要注意的是 new_size 必须大于 self.size，否则程序会崩溃
    pub fn blocks_num_needed(&self, new_size: u32) -> u32 {
        assert!(new_size >= self.size);
        Self::total_blocks(new_size) - Self::total_blocks(self.size)
    }

    // 将新的 data blocks 的 block ids 写入 inode 中，
    // new_blocks 表示新增的 block ids，如果超出了 direct/indirect1 bound 的值，
    // 需要指定 indirect1/indirect2 block 的值，假设当前 inode 的索引数据为空，
    // 那么 new_blocks 的值为：
    // [0, ..., 27, <indirect1(0) block id>, ..., <indirect2 block id>, ...,
    // <indirect1(1) block id>, ...]，
    // 其中 indirect1(x) 表示第 x 个 indirect1 block id
    pub fn increase_size(
        &mut self,
        new_size: u32,
        new_blocks: Vec<u32>,
        block_device: Arc<dyn BlockDevice>,
    ) {
        let mut current_blocks = self.data_blocks();
        self.size = new_size;
        let mut total_blocks = self.data_blocks();
        let mut new_blocks_iter = new_blocks.into_iter();
        // fill direct blocks
        while current_blocks < total_blocks.min(INODE_DIRECT_COUNT as u32) {
            self.direct[current_blocks as usize] = new_blocks_iter.next().unwrap();
            current_blocks += 1;
        }
        if total_blocks <= INODE_DIRECT_COUNT as u32 {
            return;
        }
        // alloc indirect1 block
        // if current_blocks > INODE_DIRECT_COUNT, then the value of
        // self.indirect1 will not be changed.
        if current_blocks == INODE_DIRECT_COUNT as u32 {
            self.indirect1 = new_blocks_iter.next().unwrap();
        }
        current_blocks -= INODE_DIRECT_COUNT as u32;
        total_blocks -= INODE_DIRECT_COUNT as u32;
        // fill indirect1 block
        get_block_cache(self.indirect1 as usize, block_device.clone())
            .lock()
            .modify(0, |indirect1_block: &mut IndirectBlock| {
                while current_blocks < total_blocks.min(INODE_INDIRECT1_COUNT as u32) {
                    indirect1_block[current_blocks as usize] = new_blocks_iter.next().unwrap();
                    current_blocks += 1;
                }
            });
        if total_blocks <= INODE_INDIRECT1_COUNT as u32 {
            return;
        }
        // alloc indirect2 block
        if current_blocks == INODE_INDIRECT1_COUNT as u32 {
            self.indirect2 = new_blocks_iter.next().unwrap();
        }
        current_blocks -= INODE_INDIRECT1_COUNT as u32;
        total_blocks -= INODE_INDIRECT1_COUNT as u32;
        // fill indirect2 block
        let mut a0 = current_blocks as usize / INODE_INDIRECT1_COUNT; // indirect2 current block index
        let mut b0 = current_blocks as usize % INODE_INDIRECT1_COUNT; // the first indirect1 current block index
        let a1 = total_blocks as usize / INODE_INDIRECT1_COUNT; // indirect2 total block index
        let b1 = total_blocks as usize % INODE_INDIRECT1_COUNT; // the last indirect1 total block index
        get_block_cache(self.indirect2 as usize, block_device.clone())
            .lock()
            .modify(0, |indirect2_block: &mut IndirectBlock| {
                while (a0 < a1) || (a0 == a1 && b0 < b1) {
                    if b0 == 0 {
                        indirect2_block[a0] = new_blocks_iter.next().unwrap();
                    }
                    get_block_cache(indirect2_block[a0] as usize, block_device.clone())
                        .lock()
                        .modify(0, |indirect1_block: &mut IndirectBlock| {
                            indirect1_block[b0] = new_blocks_iter.next().unwrap();
                        });
                    b0 += 1;
                    if b0 == INODE_INDIRECT1_COUNT {
                        b0 = 0;
                        a0 += 1;
                    }
                }
            });
    }

    // 将 inode 中的 data blocks 重置为空，返回需要被释放的 data block 的 block
    // ids，是 increase_size 的逆操作
    pub fn clear_size(&mut self, block_device: Arc<dyn BlockDevice>) -> Vec<u32> {
        let mut v = Vec::new();
        let mut data_blocks = self.data_blocks() as usize;
        self.size = 0;
        let mut current_block = 0usize;
        // direct
        while current_block < data_blocks.min(INODE_DIRECT_COUNT) {
            v.push(self.direct[current_block]);
            self.direct[current_block] = 0;
            current_block += 1;
        }
        if current_block <= INODE_DIRECT_COUNT {
            return v;
        }
        // indirect1
        v.push(self.indirect1);
        data_blocks -= INODE_DIRECT_COUNT;
        current_block = 0;
        get_block_cache(self.indirect1 as usize, block_device.clone())
            .lock()
            .modify(0, |indirect1_block: &mut IndirectBlock| {
                while current_block < data_blocks.min(INODE_INDIRECT1_COUNT) {
                    v.push(indirect1_block[current_block]);
                    indirect1_block[current_block] = 0;
                    current_block += 1;
                }
            });
        self.indirect1 = 0;
        if current_block <= INODE_INDIRECT1_COUNT {
            return v;
        }
        // indirect2
        v.push(self.indirect2);
        data_blocks -= INODE_INDIRECT1_COUNT;
        assert!(data_blocks <= INODE_INDIRECT2_COUNT);
        let a1 = data_blocks / INODE_INDIRECT1_COUNT; // indirect2 total block index
        let b1 = data_blocks % INODE_INDIRECT1_COUNT; // the last indirect1 total block index
        get_block_cache(self.indirect2 as usize, block_device.clone())
            .lock()
            .modify(0, |indirect2_block: &mut IndirectBlock| {
                // full indirect1 blocks
                for entry in indirect2_block.iter_mut().take(a1) {
                    v.push(*entry);
                    get_block_cache(*entry as usize, block_device.clone())
                        .lock()
                        .modify(0, |indirect1_block: &mut IndirectBlock| {
                            for entry in indirect1_block.iter() {
                                v.push(*entry);
                            }
                        });
                }
                // last indirect1 block
                if b1 > 0 {
                    v.push(indirect2_block[a1]);
                    get_block_cache(indirect2_block[a1] as usize, block_device.clone())
                        .lock()
                        .modify(0, |indirect1_block: &mut IndirectBlock| {
                            for entry in indirect1_block.iter().take(b1) {
                                v.push(*entry);
                            }
                        });
                }
            });
        self.indirect2 = 0;
        v
    }

    // 从 data block 读取数据到 buf 中，返回读取的字节数，offset 是指数据开始的
    // 位置
    pub fn read_at(
        &self,
        offset: usize,
        buf: &mut [u8],
        block_device: Arc<dyn BlockDevice>,
    ) -> usize {
        let mut start = offset;
        let end = (offset + buf.len()).min(self.size as usize);
        if start >= end {
            return 0;
        }
        let mut start_block = start / BLOCK_SIZE;
        let mut read_size = 0usize;
        loop {
            let end_current_block = ((start_block + 1) * BLOCK_SIZE).min(end);
            let block_read_size = end_current_block - start;
            let dst = &mut buf[read_size..read_size + block_read_size];
            get_block_cache(
                self.get_block_id(start_block as u32, block_device.clone()) as usize,
                block_device.clone(),
            )
            .lock()
            .read(0, |data_block: &DataBlock| {
                dst.copy_from_slice(
                    &data_block[start % BLOCK_SIZE..start % BLOCK_SIZE + block_read_size],
                );
            });
            read_size += block_read_size;
            // the max value of end_current_block is end, so if they are equal,
            // it means we have read all data.
            if end_current_block == end {
                break;
            }
            start_block += 1;
            start = end_current_block;
        }
        read_size
    }

    // 向 data block 写入数据到 buf 中，返回写入的字节数，在 write_at 方法中不会
    // 自动扩充 self.size，必须提前调用 self.increase_size 方法保证 blocks 的可
    // 用数量
    pub fn write_at(
        &mut self,
        offset: usize,
        buf: &mut [u8],
        block_device: Arc<dyn BlockDevice>,
    ) -> usize {
        let mut start = offset;
        let end = (offset + buf.len()).min(self.size as usize);
        if start >= end {
            return 0;
        }
        let mut start_block = start / BLOCK_SIZE;
        let mut write_size = 0usize;
        loop {
            let end_current_block = ((start_block + 1) * BLOCK_SIZE).min(end);
            let block_write_size = end_current_block - start;
            get_block_cache(
                self.get_block_id(start_block as u32, block_device.clone()) as usize,
                block_device.clone(),
            )
            .lock()
            .modify(0, |data_block: &mut DataBlock| {
                let src = &buf[write_size..write_size + block_write_size];
                let dst =
                    &mut data_block[start % BLOCK_SIZE..start % BLOCK_SIZE + block_write_size];
                dst.copy_from_slice(src);
            });
            write_size += block_write_size;
            if end_current_block == end {
                break;
            }
            start_block += 1;
            start = end_current_block;
        }

        write_size
    }
}

// DirEntry 表示一个目录项的基本结构，占用 32B（28B + 4B）空间
#[repr(C)]
pub struct DirEntry {
    // name 最长 27 个 char 和一个 '\0'
    name: [u8; NAME_LENGTH_LIMIT + 1],
    inode_number: u32,
}

impl DirEntry {
    pub fn empty() -> Self {
        Self {
            name: [0u8; NAME_LENGTH_LIMIT + 1],
            inode_number: 0,
        }
    }

    pub fn new(name: &str, inode_number: u32) -> Self {
        assert!(name.len() <= NAME_LENGTH_LIMIT);
        let mut bytes = [0u8; NAME_LENGTH_LIMIT + 1];
        bytes[..name.len()].copy_from_slice(name.as_bytes());
        bytes[name.len()] = '\0' as u8;
        Self {
            name: bytes,
            inode_number,
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(self as *const _ as usize as *const u8, DIR_ENTRY_SIZE)
        }
    }

    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(self as *mut _ as usize as *mut u8, DIR_ENTRY_SIZE)
        }
    }

    pub fn name(&self) -> &str {
        let len = (0usize..).find(|i| self.name[*i] == '\0' as u8).unwrap();
        core::str::from_utf8(&self.name[..len]).unwrap()
    }

    pub fn inode_number(&self) -> u32 {
        self.inode_number
    }
}
