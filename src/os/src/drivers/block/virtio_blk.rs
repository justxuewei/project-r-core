use alloc::vec::Vec;
use easy_fs::BlockDevice;
use lazy_static::*;
use virtio_drivers::{VirtIOBlk, VirtIOHeader};

use crate::{
    mm::{
        address::{PhysAddr, PhysPageNum, StepByOne, VirtAddr},
        frame_allocator::{frame_alloc, frame_dealloc},
        memory_set::kernel_token,
        page_table::PageTable,
        FrameTracker,
    },
    sync::UPSafeCell,
};

// 参见 config::MMIO
const VIRTIO0: usize = 0x10001000;

lazy_static! {
    static ref QUEUE_FRAMES: UPSafeCell<Vec<FrameTracker>> = unsafe { UPSafeCell::new(Vec::new()) };
}

// TODO(justxuewei): 查看 VirtIOBlk 是怎么实现的对块设备的抽象的？
// virtio_drivers crate 提供了 VirtIO 块设备抽象，
// VirtIO 是通过共享内存的方式实现的 qemu 和 guest 的 VirtQueue 互通。
pub struct VirtIOBlock(UPSafeCell<VirtIOBlk<'static>>);

impl VirtIOBlock {
    pub fn new() -> Self {
        unsafe {
            Self(UPSafeCell::new(
                // VirtIOHeader 代表以 MMIO 方式访问 VirtIO 设备所需的一组设备寄存器
                VirtIOBlk::new(&mut *(VIRTIO0 as *mut VirtIOHeader)).unwrap(),
            ))
        }
    }
}

impl BlockDevice for VirtIOBlock {
    fn read_block(&self, block_id: usize, buf: &mut [u8]) {
        self.0
            .exclusive_access()
            .read_block(block_id, buf)
            .expect("Error when reading VirtIOBlk");
    }

    fn write_block(&self, block_id: usize, buf: &[u8]) {
        self.0
            .exclusive_access()
            .write_block(block_id, buf)
            .expect("Error when reading VirtIOBlk");
    }
}

// 在 virtio_drivers crate 中定义了如下接口，需要在 os 中实现
// https://github.com/rcore-os/virtio-drivers/blob/master/src/hal.rs#L57
// extern "C" {
//    fn virtio_dma_alloc(pages: usize) -> PhysAddr;
//    fn virtio_dma_dealloc(paddr: PhysAddr, pages: usize) -> i32;
//    fn virtio_phys_to_virt(paddr: PhysAddr) -> VirtAddr;
//    fn virtio_virt_to_phys(vaddr: VirtAddr) -> PhysAddr;
// }

#[no_mangle]
pub extern "C" fn virtio_dma_alloc(pages: usize) -> PhysAddr {
    let mut ppn_base = PhysPageNum(0);
    for i in 0..pages {
        let frame = frame_alloc().unwrap();
        if i == 0 {
            ppn_base = frame.ppn;
        }
        // VirtQueue 是一个环形结构，它的申请和释放都是线性的
        assert_eq!(frame.ppn.0, ppn_base.0 + i);
        QUEUE_FRAMES.exclusive_access().push(frame);
    }
    ppn_base.into()
}

#[no_mangle]
pub extern "C" fn virtio_dma_dealloc(pa: PhysAddr, pages: usize) -> i32 {
    let mut ppn_base: PhysPageNum = pa.into();
    for _ in 0..pages {
        frame_dealloc(ppn_base);
        ppn_base.step();
    }
    0
}

#[no_mangle]
pub extern "C" fn virtio_phys_to_virt(pa: PhysAddr) -> VirtAddr {
    VirtAddr(pa.0)
}

#[no_mangle]
pub extern "C" fn virtio_virt_to_phys(va: VirtAddr) -> PhysAddr {
    PageTable::from_token(kernel_token())
        .translate_va(va)
        .unwrap()
}
