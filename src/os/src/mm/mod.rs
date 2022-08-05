pub mod address;
pub mod frame_allocator;
mod heap_allocator;
pub mod memory_set;
pub mod page_table;

pub use memory_set::KERNEL_SPACE;
pub use page_table::UserBuffer;
pub use frame_allocator::FrameTracker;

pub fn init() {
    heap_allocator::init_heap();
    frame_allocator::init_frame_allocator();
    KERNEL_SPACE.exclusive_access().activate();
}
