use super::{PhysAddr, PhysPageNum};
use alloc::vec::Vec;
use spin::Mutex;
use crate::config::MEMORY_END;
use lazy_static::*;
use core::fmt::{self, Debug, Formatter};

pub struct FrameTracker {
    pub ppn: PhysPageNum,
}

impl FrameTracker {
    pub fn new(ppn: PhysPageNum) -> Self {
        // page cleaning
        let bytes_array = ppn.get_bytes_array();
        // 所有字节清零
        for i in bytes_array {
            *i = 0;
        }
        Self { ppn }
    }
}

impl Debug for FrameTracker {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("FrameTracker:PPN={:#x}", self.ppn.0))
    }
}

// 当一个 FrameTracker 生命周期结束被编译器回收的时候，我们需要将它控制的物理页帧回收掉 FRAME_ALLOCATOR 中
impl Drop for FrameTracker {
    fn drop(&mut self) {
        frame_dealloc(self.ppn);
    }
}

// 以物理页号为单位进行物理页帧的分配和回收
trait FrameAllocator {
    fn new() -> Self;
    fn alloc(&mut self) -> Option<PhysPageNum>;
    fn dealloc(&mut self, ppn: PhysPageNum);
}

// 栈式物理页帧管理策略
// 物理页号区间 [current,end) 此前均 从未 被分配出去过
// recycled 以 后入先出 的方式保存了被回收的物理页号
pub struct StackFrameAllocator {
    current: usize,
    end: usize,
    recycled: Vec<usize>,
}


impl StackFrameAllocator {
    // 真正被使用起来之前，需要调用 init 方法将自身的 [current,end) 初始化为可用物理页号区间
    pub fn init(&mut self, l: PhysPageNum, r: PhysPageNum) {
        self.current = l.0;
        self.end = r.0;
        info!("[kernel] last {} Physical Frames.", self.end - self.current);
    }
    fn usable_frames(&self) -> usize {
        self.end - self.current + self.recycled.len()
    }
}

impl FrameAllocator for StackFrameAllocator {
    // 只需将区间两端均设为 0 ， 然后创建一个新的向量
    fn new() -> Self {
        Self {
            current: 0,
            end: 0,
            recycled: Vec::new(),
        }
    }
    // 物理页帧分配
    fn alloc(&mut self) -> Option<PhysPageNum> {
        // 检查栈 recycled 内有没有之前回收的物理页号，如果有的话直接弹出栈顶并返回
        if let Some(ppn) = self.recycled.pop() {
            // println!("[kernel] alloc one frame. l:r={}:{}", self.current, self.end);
            Some(ppn.into())
        } else {
            // 从之前从未分配过的物理页号区间 [current,end) 上进行分配，
            // 我们分配它的 左端点 current ，同时将管理器内部维护的 current 加一代表 current 此前已经被分配过了
            if self.current == self.end { // 内存耗尽分配失败
                // println!("[kernel] Out Of Memory! No physical frames can be allocated! l=r={}", self.current);
                None
            } else {
                // println!("[kernel] alloc one frame. l:r={}:{}", self.current, self.end);
                self.current += 1;
                Some((self.current - 1).into())
            }
        }
    }
    fn dealloc(&mut self, ppn: PhysPageNum) {
        let ppn = ppn.0;
        // 检查回收页面的合法性
        // validity check
        // NOTE: self.recycled 中的元素一定小于 self.current
        if ppn >= self.current || self.recycled
            .iter()
            .find(|&v| {*v == ppn})
            .is_some() {
            panic!("Frame ppn={:#x} has not been allocated!", ppn);
        }
        // recycle
        self.recycled.push(ppn);
    }
}

type FrameAllocatorImpl = StackFrameAllocator;

// StackFrameAllocator 的全局实例
lazy_static! {
    pub static ref FRAME_ALLOCATOR: Mutex<FrameAllocatorImpl> =
        Mutex::new(FrameAllocatorImpl::new());
}

pub fn init_frame_allocator() {
    extern "C" {
        fn ekernel();
    }
    // 调用物理地址 PhysAddr 的 floor/ceil 方法分别下/上取整获得可用的物理页号区间
    // 可用的物理内存对应的物理页号: [ekernel.ceil(), MEMORY_END.floor())
    FRAME_ALLOCATOR
        .lock()
        .init(PhysAddr::from(ekernel as usize).ceil(), PhysAddr::from(MEMORY_END).floor());
    info!("[kernel] Frame Total Size [{:#x}, {:#x})", ekernel as usize, MEMORY_END);
}

// 包装为一个 FrameTracker
// 将一个物理页帧的生命周期绑定到一个 FrameTracker 变量上，
// 当一个 FrameTracker 被创建的时候，我们需要从 FRAME_ALLOCATOR 中分配一个 被清零的物理页帧
pub fn frame_alloc() -> Option<FrameTracker> {
    // println!("[kernel] alloc one frame.");
    FRAME_ALLOCATOR
        .lock()
        .alloc()
        .map(|ppn| FrameTracker::new(ppn))
}

pub fn frame_dealloc(ppn: PhysPageNum) {
    FRAME_ALLOCATOR
        .lock()
        .dealloc(ppn);
}

pub fn usable_frames() -> usize {
    FRAME_ALLOCATOR
        .lock()
        .usable_frames()
}

#[allow(unused)]
pub fn frame_allocator_test() {
    let mut v: Vec<FrameTracker> = Vec::new();
    for i in 0..5 {
        let frame = frame_alloc().unwrap();
        println!("{:?}", frame);
        v.push(frame);
    }
    v.clear(); // 在这里回收
    for i in 0..5 {
        let frame = frame_alloc().unwrap();
        println!("{:?}", frame);
        v.push(frame);
    }
    drop(v); // 在这里回收
    println!("frame_allocator_test passed!");
}
