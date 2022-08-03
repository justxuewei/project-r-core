use easy_fs::BlockDevice;
use virtio_drivers::{VirtIOBlk, VirtIOHeader};

use crate::sync::UPSafeCell;

const VIRTIO0: usize = 0x10001000;

// TODO(justxuewei): 查看 VirtIOBlk 是怎么实现的
pub struct VirtIOBlock(UPSafeCell<VirtIOBlk<'static>>);

impl VirtIOBlock {
    pub fn new() -> Self {
        unsafe {
            Self(UPSafeCell::new(
                VirtIOBlk::new(&mut *(VIRTIO0 as *mut VirtIOHeader)).unwrap(),
            ))
        }
    }
}

impl BlockDevice for VirtIOBlock {
    fn read_block(&self, block_id: usize, buf: &mut [u8]) {
        panic!("todo")
    }

    fn write_block(&self, block_id: usize, buf: &[u8]) {
        panic!("todo")
    }
}
