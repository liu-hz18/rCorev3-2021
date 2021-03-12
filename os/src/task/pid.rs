use alloc::vec::Vec;
use lazy_static::*;
use spin::Mutex;
use crate::mm::{KERNEL_SPACE, MapPermission, VirtAddr};
use crate::config::{
    PAGE_SIZE,
    TRAMPOLINE,
    KERNEL_STACK_SIZE,
};

struct PidAllocator {
    current: usize,
    recycled: Vec<usize>,
}

// 简单栈式分配策略的进程标识符分配器 PidAllocator
impl PidAllocator {
    pub fn new() -> Self {
        PidAllocator {
            current: 0,
            recycled: Vec::new(),
        }
    }
    pub fn alloc(&mut self) -> PidHandle {
        if let Some(pid) = self.recycled.pop() {
            PidHandle(pid)
        } else {
            self.current += 1;
            PidHandle(self.current - 1)
        }
    }
    pub fn dealloc(&mut self, pid: usize) {
        assert!(pid < self.current);
        assert!(
            self.recycled.iter().find(|ppid| **ppid == pid).is_none(),
            "pid {} has been deallocated!", pid
        );
        self.recycled.push(pid);
    }
}

lazy_static! {
    static ref PID_ALLOCATOR : Mutex<PidAllocator> = Mutex::new(PidAllocator::new());
}

// 进程标识符, 互不相同的整数
// RAII: 当它的生命周期结束后对应的整数会被编译器自动回收
#[derive(Debug)]
pub struct PidHandle(pub usize);

// RAII: 允许编译器进行自动的资源回收
impl Drop for PidHandle {
    fn drop(&mut self) {
        //println!("drop pid {}", self.0);
        PID_ALLOCATOR.lock().dealloc(self.0);
    }
}

// 全局分配进程标识符的接口
pub fn pid_alloc() -> PidHandle {
    PID_ALLOCATOR.lock().alloc()
}

// 根据进程标识符计算内核栈在内核地址空间中的位置
/// Return (bottom, top) of a kernel stack in kernel space.
pub fn kernel_stack_position(app_id: usize) -> (usize, usize) {
    let top = TRAMPOLINE - app_id * (KERNEL_STACK_SIZE + PAGE_SIZE);
    let bottom = top - KERNEL_STACK_SIZE;
    (bottom, top)
}

// 内核栈
#[derive(Debug)]
pub struct KernelStack {
    pid: usize,
}

impl KernelStack {
    // 从一个 PidHandle ，也就是一个已分配的进程标识符中对应生成一个内核栈 KernelStack
    pub fn new(pid_handle: &PidHandle) -> Self {
        let pid = pid_handle.0;
        let (kernel_stack_bottom, kernel_stack_top) = kernel_stack_position(pid);
        KERNEL_SPACE
            .lock()
            .insert_framed_area(
                kernel_stack_bottom.into(),
                kernel_stack_top.into(),
                MapPermission::R | MapPermission::W,
            );
        KernelStack {
            pid: pid_handle.0,
        }
    }
    // 将一个类型为 T 的变量压入内核栈顶并返回其裸指针，这也是一个泛型函数
    pub fn push_on_top<T>(&self, value: T) -> *mut T where
        T: Sized, {
        let kernel_stack_top = self.get_top();
        let ptr_mut = (kernel_stack_top - core::mem::size_of::<T>()) as *mut T;
        unsafe { *ptr_mut = value; }
        ptr_mut
    }
    // 获取当前内核栈顶在内核地址空间中的地址
    pub fn get_top(&self) -> usize {
        let (_, kernel_stack_top) = kernel_stack_position(self.pid);
        kernel_stack_top
    }
}

// RAII: 实际保存它的物理页帧的生命周期被绑定到它下面，当 KernelStack 生命周期结束后，这些物理页帧也将会被编译器自动回收
impl Drop for KernelStack {
    fn drop(&mut self) {
        let (kernel_stack_bottom, _) = kernel_stack_position(self.pid);
        let kernel_stack_bottom_va: VirtAddr = kernel_stack_bottom.into();
        // 在内核地址空间中将对应的逻辑段删除
        // 意味着那些物理页帧被同时回收掉了
        KERNEL_SPACE
            .lock()
            .remove_area_with_start_vpn(kernel_stack_bottom_va.into());
    }
}
