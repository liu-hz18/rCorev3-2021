mod heap_allocator;
mod address;
mod frame_allocator;
mod page_table;
mod memory_set;

use page_table::{PageTable, PTEFlags};
pub use address::{VPNRange, StepByOne};
pub use address::{PhysAddr, VirtAddr, PhysPageNum, VirtPageNum};
pub use frame_allocator::{FrameTracker, frame_alloc, usable_frames};
pub use page_table::{
    PageTableEntry,
    translated_byte_buffer,
    translated_virtual_ptr,
    virtual_addr_writable,
    virtual_addr_printable,
    virtual_addr_range_printable,
    virtual_addr_range_writable,
    translated_str,
    translated_refmut,
    UserBuffer,
    UserBufferIterator,
};
pub use memory_set::{MemorySet, KERNEL_SPACE, MapPermission, MapArea, MapType};
pub use memory_set::remap_test;

pub fn init() {
    // 全局动态内存分配器的初始化
    heap_allocator::init_heap();
    // 初始化物理页帧 管理器, 内含堆数据结构 Vec<T>
    frame_allocator::init_frame_allocator();
    // 创建内核地址空间并让 CPU 开启分页模式, MMU 在地址转换的时候使用内核的多级页表
    // 这是 KERNEL_SPACE 第一次被使用
    KERNEL_SPACE.lock().activate();
}
