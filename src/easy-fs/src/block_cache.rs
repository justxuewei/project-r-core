use alloc::{collections::VecDeque, sync::Arc};
use lazy_static::*;
use spin::Mutex;

use crate::block_dev::BlockDevice;

// 一个 block 的长度是 512B
pub const BLOCK_SIZE: usize = 512;
const BLOCK_CACHE_SIZE: usize = 16;

lazy_static! {
    pub static ref BLOCK_CACHE_MANAGER: Mutex<BlockCacheManager> =
        Mutex::new(BlockCacheManager::new());
}

pub fn get_block_cache(
    block_id: usize,
    block_device: Arc<dyn BlockDevice>,
) -> Arc<Mutex<BlockCache>> {
    BLOCK_CACHE_MANAGER
        .lock()
        .get_block_cache(block_id, block_device)
}

// 强制将缓冲区的数据写回到磁盘上
pub fn block_cache_sync_all() {
    for (_, cache) in BLOCK_CACHE_MANAGER.lock().queue.iter() {
        cache.lock().sync();
    }
}

pub struct BlockCache {
    cache: [u8; BLOCK_SIZE],
    block_id: usize,
    block_device: Arc<dyn BlockDevice>,
    modified: bool,
}

impl BlockCache {
    // 创建一个 block cache 并从 block device 中缓存最新的数据
    pub fn new(block_id: usize, block_device: Arc<dyn BlockDevice>) -> Self {
        let mut cache = [0; BLOCK_SIZE];
        block_device.read_block(block_id, &mut cache);
        Self {
            cache,
            block_id,
            block_device,
            modified: false,
        }
    }

    // 获取块内偏移地址
    fn addr_of_offset(&self, offset: usize) -> usize {
        &self.cache[offset] as *const _ as usize
    }

    // 在 offset 位置获取结构体 T 的引用
    pub fn get_ref<T>(&self, offset: usize) -> &T
    where
        T: Sized,
    {
        let size = core::mem::size_of::<T>();
        assert!(offset + size <= BLOCK_SIZE);
        unsafe { &*(self.addr_of_offset(offset) as *const T) }
    }

    // 在 offset 位置获取结构体 T 的可变引用
    pub fn get_mut<T>(&mut self, offset: usize) -> &mut T
    where
        T: Sized,
    {
        let size = core::mem::size_of::<T>();
        assert!(offset + size <= BLOCK_SIZE);
        self.modified = true;
        unsafe { &mut *(self.addr_of_offset(offset) as *mut T) }
    }

    // read 方法是对 get_ref 的进一步封装
    pub fn read<T, V>(&self, offset: usize, f: impl FnOnce(&T) -> V) -> V {
        f(self.get_ref::<T>(offset))
    }

    // modify 方法是对 get_mut 的进一步封装
    pub fn modify<T, V>(&mut self, offset: usize, f: impl FnOnce(&mut T) -> V) -> V {
        f(self.get_mut::<T>(offset))
    }

    // 将内存缓冲区数据写回 block device
    fn sync(&mut self) {
        if self.modified {
            self.modified = false;
            self.block_device.write_block(self.block_id, &self.cache)
        }
    }
}

// 如果 block cache 被回收的时候，将最新的结果写回到 block device
impl Drop for BlockCache {
    fn drop(&mut self) {
        self.sync();
    }
}

pub struct BlockCacheManager {
    queue: VecDeque<(usize, Arc<Mutex<BlockCache>>)>,
}

impl BlockCacheManager {
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
        }
    }

    // 获取 block cache，如果 block cache 不存在则会在队列中新增/替换一个旧
    // block cache
    pub fn get_block_cache(
        &mut self,
        block_id: usize,
        block_device: Arc<dyn BlockDevice>,
    ) -> Arc<Mutex<BlockCache>> {
        // try to get block cache from memory
        let existed = self.queue.iter().find(|(id, _)| *id == block_id);
        if let Some(block_cache) = existed {
            return block_cache.1.clone();
        }
        // block cache not existed on the memory
        assert!(self.queue.len() <= BLOCK_CACHE_SIZE);
        // try to replace on old block cache
        if self.queue.len() == BLOCK_CACHE_SIZE {
            if let Some((idx, _)) = self
                .queue
                .iter()
                .enumerate()
                .find(|(_, pair)| Arc::strong_count(&pair.1) == 1)
            {
                self.queue.drain(idx..=idx);
            } else {
                panic!("Block cache queue is full");
            }
        }
        // create a new block cache
        let block_cache = Arc::new(Mutex::new(BlockCache::new(block_id, block_device.clone())));
        self.queue.push_back((block_id, block_cache.clone()));
        block_cache
    }
}
