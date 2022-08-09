mod virtio_blk;

use alloc::sync::Arc;
use easy_fs::BlockDevice;
use lazy_static::*;

use crate::drivers::block::virtio_blk::VirtIOBlock;

lazy_static! {
    pub static ref BLOCK_DEVICE: Arc<dyn BlockDevice> = Arc::new(VirtIOBlock::new());
}
