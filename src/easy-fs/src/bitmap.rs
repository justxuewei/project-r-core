use alloc::sync::Arc;

use crate::{
    block_cache::{get_block_cache, BLOCK_SIZE},
    block_dev::BlockDevice,
};

// 一个 block 包含的 bits 的数量
pub const BLOCK_BITS: usize = BLOCK_SIZE * 8;
const BITMAP_SIZE: usize = 64;
const BITMAP_BLOCK_SIZE: usize = 64;

// BitmapBlock 表示一个在 block device 上的 bitmap block，easyfs 的 block 的
// size 为 512B，所以 bitmap block 的长度为 8B * 64 = 512B。
type BitmapBlock = [u64; BITMAP_BLOCK_SIZE];

// Bitmap 表示一个具体的 bitmap 在 block device 中的位置，需要注意的是该结构是保
// 存于 mem 中的，需要去 block device 中读取实际的 bitmap 的值。
pub struct Bitmap {
    // 起始 block id
    start_block_id: usize,
    // blocks 的数量
    blocks: usize,
}

impl Bitmap {
    pub fn new(start_block_id: usize, blocks: usize) -> Self {
        Self {
            start_block_id,
            blocks,
        }
    }

    // 在 bitmap 中找到一个空闲位置并标记占用，返回 (inode/data) area 的 block id
    pub fn alloc(&self, block_device: Arc<dyn BlockDevice>) -> Option<usize> {
        // iterate all bitmap blocks
        for bitmap_block_id in 0..self.blocks {
            let bit = get_block_cache(self.start_block_id + bitmap_block_id, block_device.clone())
                .lock()
                .modify(0, |bitmap_block: &mut BitmapBlock| {
                    // pos is offset in BitmapBlock, inner_pos is offset in a
                    // bitmap (u64)
                    let pair = bitmap_block
                        .iter()
                        .enumerate()
                        .find(|(_, bitmap)| **bitmap != u64::MAX)
                        .map(|(pos, bitmap)| (pos, bitmap.trailing_ones() as usize));

                    if let Some((pos, inner_pos)) = pair {
                        bitmap_block[pos] |= 1 << inner_pos;
                        // TEMP(justxuewei):
                        // - bitmap_block_id + self.start_block_id 是 bitmap 所
                        //   在的 block id；
                        // - pos 是 bitmap block 内部的 bitmap id；
                        // - inner_pos 是 bitmap 内部的偏移量；
                        // - 观察 bitmap_block[pos] 的哪个位置置为 1。
                        return Some(
                            (bitmap_block_id * BLOCK_BITS + pos * BITMAP_SIZE + inner_pos) as usize,
                        );
                    }
                    None
                });
            if bit.is_some() {
                return bit;
            }
        }
        None
    }

    pub fn dealloc(&self, block_device: Arc<dyn BlockDevice>, bit: usize) {
        let (block_id, bitmap_pos, bit_pos) = decomposition(bit);
        get_block_cache(block_id + self.start_block_id, block_device)
            .lock()
            .modify(0, |bitmap_block: &mut BitmapBlock| {
                assert!((bitmap_block[bitmap_pos] & (1u64 << bit_pos)) > 0);
                bitmap_block[bitmap_pos] -= 1u64 << bit_pos;
            });
    }

    pub fn maximum(&self) -> usize {
        self.blocks * BLOCK_BITS
    }
}

// 将 bit 的位置（pos）分解为 block_id, bitmap_pos, bit_pos，是 alloc 操作的反操
// 作
fn decomposition(bit: usize) -> (usize, usize, usize) {
    let block_id = bit / BLOCK_BITS;
    let bitmap_pos = (bit % BLOCK_BITS) / 64;
    let bit_pos = (bit % BLOCK_BITS) % 64;
    (block_id, bitmap_pos, bit_pos)
}
