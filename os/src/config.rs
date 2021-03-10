pub const USER_STACK_SIZE: usize = 4096;
pub const KERNEL_STACK_SIZE: usize = 4096 * 2;

// 页表&虚存机制
pub const KERNEL_HEAP_SIZE: usize = 0x30_0000;
pub const MEMORY_END: usize = 0x80800000; // 硬编码整块物理内存的终止物理地址为 0x80800000, 可用内存大小设置为 8MiB 
pub const PAGE_SIZE: usize = 0x1000;
pub const PAGE_SIZE_BITS: usize = 0xc;
// 可用的物理内存对应的物理页号: [ekernel.ceil(), MEMORY_END.floor())

pub const TRAMPOLINE: usize = usize::MAX - PAGE_SIZE + 1;
pub const TRAP_CONTEXT: usize = TRAMPOLINE - PAGE_SIZE;
/// Return (bottom, top) of a kernel stack in kernel space.
pub fn kernel_stack_position(app_id: usize) -> (usize, usize) {
    let top = TRAMPOLINE - app_id * (KERNEL_STACK_SIZE + PAGE_SIZE);
    let bottom = top - KERNEL_STACK_SIZE;
    (bottom, top)
}

pub const CLOCK_FREQ: usize = 12500000;

// Stride 调度
pub const BIG_STRIDE: isize = 0x7FFFFFFF;
pub const TASK_INIT_PRIORITY: isize = 16;
