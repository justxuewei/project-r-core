use alloc::sync::Arc;
use spin::Mutex;

use crate::{
    block_cache::get_block_cache,
    block_dev::BlockDevice,
    efs::EasyFileSystem,
    layout::{DirEntry, DiskInode, DIR_ENTRY_SIZE},
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

    // 查找一个文件名的 inode，需要注意的是当前的 Inode 必须为一个文件夹（DiskInodeType::Dir）
    // TODO(justxuewei): 整理下磁盘的结构图
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
}
