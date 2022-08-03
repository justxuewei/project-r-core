pub const USER_STACK_SIZE: usize = 4096 * 2;
pub const KERNEL_STACK_SIZE: usize = 4096 * 2;
pub const KERNEL_HEAP_SIZE: usize = 0x30_0000;

pub const MEMORY_END: usize = 0x80800000;

// address space
pub const PAGE_SIZE: usize = 0x1000;
pub const PAGE_SIZE_BITS: usize = 0xc;

// high kernel/application address space
// Ref: https://rcore-os.github.io/rCore-Tutorial-Book-v3/chapter4/5kernel-app-spaces.html#id6
pub const TRAMPOLINE: usize = usize::MAX - PAGE_SIZE + 1;
pub const TRAP_CONTEXT: usize = TRAMPOLINE - PAGE_SIZE;

// CLOCK_FREQ is clock frequency, in this case, the value is for qemu.
pub const CLOCK_FREQ: usize = 12500000;

// MMIO for QEMU
// the first one is started address, the second one is length
pub const MMIO: &[(usize, usize)] = &[(0x10001000, 0x1000)];
