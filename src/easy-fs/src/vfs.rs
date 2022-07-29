use alloc::{string::String, sync::Arc, vec::Vec};
use spin::{Mutex, MutexGuard};

use crate::{
    block_cache::{block_cache_sync_all, get_block_cache},
    block_dev::BlockDevice,
    efs::EasyFileSystem,
    layout::{DirEntry, DiskInode, DiskInodeType, DIR_ENTRY_SIZE},
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

    // 读取 inode 对应的 disk inode
    fn read_disk_inode<V>(&self, f: impl FnOnce(&DiskInode) -> V) -> V {
        get_block_cache(self.block_id, self.block_device.clone())
            .lock()
            .read(self.block_offset, f)
    }

    // 修改 inode 对应的 disk inode
    fn modify_disk_inode<V>(&self, f: impl FnOnce(&mut DiskInode) -> V) -> V {
        get_block_cache(self.block_id, self.block_device.clone())
            .lock()
            .modify(self.block_offset, f)
    }

    // 查找一个文件名的 inode，仅 root dir 可调用
    fn find(&self, name: &str) -> Option<Arc<Inode>> {
        let fs = self.fs.lock();
        self.read_disk_inode(|disk_inode: &DiskInode| {
            self.find_inode_id(name, disk_inode)
                .map(|inode_number: u32| {
                    let (block_id, block_offset) = fs.get_disk_inode_pos(inode_number);
                    Arc::new(Self::new(
                        block_id,
                        block_offset,
                        self.fs.clone(),
                        self.block_device.clone(),
                    ))
                })
        })
    }

    // 在一个指定的 disk inode 中遍历 data block 中的 directory entries，如果找
    // 到与 name 一样的文件则返回 inode id
    fn find_inode_id(&self, name: &str, disk_inode: &DiskInode) -> Option<u32> {
        assert!(disk_inode.is_dir());
        let file_count = (disk_inode.size as usize) / DIR_ENTRY_SIZE;
        let mut dirent = DirEntry::empty();
        for i in 0..file_count {
            assert_eq!(
                disk_inode.read_at(
                    DIR_ENTRY_SIZE * i,
                    dirent.as_bytes_mut(),
                    self.block_device.clone()
                ),
                DIR_ENTRY_SIZE
            );
            if dirent.name() == name {
                return Some(dirent.inode_number());
            }
        }
        None
    }

    // 遍历目录的文件，仅 root dir 可调用
    pub fn ls(&self) -> Vec<String> {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode: &DiskInode| {
            let file_count = (disk_inode.size as usize) / DIR_ENTRY_SIZE;
            let mut filenames = Vec::new();
            let mut dirent = DirEntry::empty();
            for i in 0..file_count {
                assert_eq!(
                    disk_inode.read_at(
                        DIR_ENTRY_SIZE * i,
                        dirent.as_bytes_mut(),
                        self.block_device.clone()
                    ),
                    DIR_ENTRY_SIZE
                );
                filenames.push(String::from(dirent.name()));
            }
            filenames
        })
    }

    // 创建一个文件，仅 root dir 可调用
    pub fn create(&self, name: &str) -> Option<Arc<Inode>> {
        let mut fs = self.fs.lock();
        // check if the name was existed
        if self
            .read_disk_inode(|root_inode: &DiskInode| {
                assert!(root_inode.is_dir());
                self.find_inode_id(name, root_inode)
            })
            .is_some()
        {
            return None;
        }
        // create an inode for the new file
        let inode_number = fs.alloc_inode();
        let (block_id, block_offset) = fs.get_disk_inode_pos(inode_number);
        get_block_cache(block_id as usize, self.block_device.clone())
            .lock()
            .modify(block_offset, |disk_inode: &mut DiskInode| {
                disk_inode.initialize(DiskInodeType::File);
            });
        // append a directory entry for the new file to the root dir
        self.modify_disk_inode(|root_disk_inode: &mut DiskInode| {
            let offset = root_disk_inode.size;
            let new_size = root_disk_inode.size + DIR_ENTRY_SIZE as u32;
            self.increase_size(new_size, root_disk_inode, &mut fs);
            let dirent = DirEntry::new(name, inode_number);
            root_disk_inode.write_at(
                offset as usize,
                dirent.as_bytes(),
                self.block_device.clone(),
            );
        });
        block_cache_sync_all();
        Some(Arc::new(Self::new(
            block_id,
            block_offset,
            self.fs.clone(),
            self.block_device.clone(),
        )))
    }

    // 清空一个文件
    pub fn clear(&self) {
        let mut fs = self.fs.lock();
        self.modify_disk_inode(|disk_inode: &mut DiskInode| {
            let size = disk_inode.size;
            let data_blocks_dealloc = disk_inode.clear_size(self.block_device.clone());
            assert!(data_blocks_dealloc.len() == DiskInode::total_blocks(size) as usize);
            for data_block in data_blocks_dealloc.into_iter() {
                fs.dealloc_data(data_block);
            }
        });
    }

    // 读取一些数据
    pub fn read_at(&self, offset: usize, buf: &mut [u8]) -> usize {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            disk_inode.read_at(offset, buf, self.block_device.clone())
        })
    }

    // 写入一些数据
    pub fn write_at(&self, offset: usize, buf: &[u8]) -> usize {
        let mut fs = self.fs.lock();
        self.modify_disk_inode(|disk_inode| {
            self.increase_size((offset + buf.len()) as u32, disk_inode, &mut fs);
            disk_inode.write_at(offset, buf, self.block_device.clone())
        })
    }

    // 增加 inode 的 size
    fn increase_size(
        &self,
        new_size: u32,
        disk_inode: &mut DiskInode,
        fs: &mut MutexGuard<EasyFileSystem>,
    ) {
        if new_size < disk_inode.size {
            return;
        }
        let blocks_needed = disk_inode.blocks_num_needed(new_size);
        let mut new_blocks: Vec<u32> = Vec::new();
        for _ in 0..blocks_needed {
            new_blocks.push(fs.alloc_data())
        }
        disk_inode.increase_size(new_size, new_blocks, self.block_device.clone())
    }
}
