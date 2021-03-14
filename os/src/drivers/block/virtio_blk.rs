
use virtio_drivers::{VirtIOBlk, VirtIOHeader};
use crate::mm::{
    PhysAddr,
    VirtAddr,
    frame_alloc,
    frame_dealloc,
    PhysPageNum,
    FrameTracker,
    StepByOne,
    PageTable,
    kernel_token,
};
use super::BlockDevice;
use spin::Mutex;
use alloc::vec::Vec;
use lazy_static::*;

#[allow(unused)]
const VIRTIO0: usize = 0x10001000;

// VirtIO 块设备抽象
pub struct VirtIOBlock(Mutex<VirtIOBlk<'static>>);

lazy_static! {
    static ref QUEUE_FRAMES: Mutex<Vec<FrameTracker>> = Mutex::new(Vec::new());
}

impl BlockDevice for VirtIOBlock {
    fn read_block(&self, block_id: usize, buf: &mut [u8]) {
        self.0.lock().read_block(block_id, buf).expect("Error when reading VirtIOBlk");
    }
    fn write_block(&self, block_id: usize, buf: &[u8]) {
        self.0.lock().write_block(block_id, buf).expect("Error when writing VirtIOBlk");
    }
}

impl VirtIOBlock {
    #[allow(unused)]
    pub fn new() -> Self {
        // VirtIOHeader 实际上就代表以 MMIO 方式访问 VirtIO 设备所需的一组设备寄存器
        // Virtio MMIO 区间左端 VIRTIO0 开始转化为一个 &mut VirtIOHeader 就可以在该平台上访问这些设备寄存器了
        Self(Mutex::new(VirtIOBlk::new(
            unsafe { &mut *(VIRTIO0 as *mut VirtIOHeader) }
        ).unwrap()))
    }
}

// 在 VirtIO 架构下，需要在公共区域中放置一种叫做 VirtQueue 的环形队列，CPU 可以向此环形队列中向 VirtIO 设备提交请求，也可以从队列中取得请求的结果
// 但这并不在 VirtIO 驱动 virtio-drivers 的职责范围之内，因此它声明了数个相关的接口，需要库的使用者自己来实现
#[no_mangle]
pub extern "C" fn virtio_dma_alloc(pages: usize) -> PhysAddr {
    let mut ppn_base = PhysPageNum(0);
    // 需要分配/回收数个 连续 的物理页帧
    // 而我们的 frame_alloc 是逐个分配，严格来说并不保证分配的连续性
    // 幸运的是，这个过程只会发生在内核初始化阶段，因此能够保证连续性
    for i in 0..pages {
        let frame = frame_alloc().unwrap();
        if i == 0 { ppn_base = frame.ppn; }
        assert_eq!(frame.ppn.0, ppn_base.0 + i);
        // 通过 frame_alloc 得到的那些物理页帧 FrameTracker 都会被保存在全局的向量 QUEUE_FRAMES 以延长它们的生命周期，避免提前被回收
        QUEUE_FRAMES.lock().push(frame);
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
pub extern "C" fn virtio_phys_to_virt(paddr: PhysAddr) -> VirtAddr {
    VirtAddr(paddr.0)
}

#[no_mangle]
pub extern "C" fn virtio_virt_to_phys(vaddr: VirtAddr) -> PhysAddr {
    PageTable::from_token(kernel_token()).translate_va(vaddr).unwrap()
}
