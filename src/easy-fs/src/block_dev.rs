use core::any::Any;

// A trait for block device, which is implemented by OS or other libraries/applications.
pub trait BlockDevice: Send + Sync + Any {
    fn read_block(&self, block_id: usize, buf: &mut [u8]);
    fn write_block(&self, block_id: usize, buf: &[u8]);
}
