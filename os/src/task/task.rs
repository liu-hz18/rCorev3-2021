// 把应用程序的一个计算阶段的执行过程（也是一段执行流）称为一个 任务
use crate::mm::{MemorySet, MapPermission, PhysPageNum, KERNEL_SPACE, VirtAddr};
use crate::trap::{TrapContext, trap_handler};
use crate::config::{BIG_STRIDE, TASK_INIT_PRIORITY, TRAP_CONTEXT, kernel_stack_position};
use super::TaskContext;


// 管理程序的执行过程的任务上下文，控制程序的执行与暂停
pub struct TaskControlBlock {
    pub task_cx_ptr: usize,
    pub task_status: TaskStatus,
    pub task_stride: isize,
    pub task_priority: isize,
    pub task_run_duration_ms: usize,
    pub task_last_start_time: usize,
    pub memory_set: MemorySet, // 应用的地址空间 
    pub trap_cx_ppn: PhysPageNum, // 位于应用地址空间次高页的 Trap 上下文被实际存放在物理页帧的物理页号
    pub base_size: usize, // 应用数据的大小，也就是 在应用地址空间中从 0x0 开始到用户栈结束一共包含多少字节
}

impl TaskControlBlock {
    pub fn get_task_cx_ptr2(&self) -> *const usize {
        &self.task_cx_ptr as *const usize
    }
    pub fn get_trap_cx(&self) -> &'static mut TrapContext {
        self.trap_cx_ppn.get_mut() // T=TrapContext here.
    }
    pub fn get_task_pass(&self) -> isize {
        BIG_STRIDE / self.task_priority
    }
    pub fn get_user_token(&self) -> usize {
        self.memory_set.token()
    }
    pub fn new(elf_data: &[u8], app_id: usize) -> Self {
        // memory_set with elf program headers/trampoline/trap context/user stack
        // 解析传入的 ELF 格式数据构造应用的地址空间 memory_set 并获得其他信息
        let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
        // 地址空间 memory_set 中查多级页表找到应用地址空间中的 Trap 上下文实际被放在哪个物理页帧
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT).into())
            .unwrap()
            .ppn();
        let task_status = TaskStatus::Ready;
        // map a kernel-stack in kernel space
        // 我们根据传入的应用 ID app_id 调用在 config 子模块中定义的 kernel_stack_position 找到 应用的内核栈预计放在内核地址空间 KERNEL_SPACE 中的哪个位置，
        // 并通过 insert_framed_area 实际将这个逻辑段 加入到内核地址空间中
        let (kernel_stack_bottom, kernel_stack_top) = kernel_stack_position(app_id);
        KERNEL_SPACE
            .lock()
            .insert_framed_area(
                kernel_stack_bottom.into(),
                kernel_stack_top.into(),
                MapPermission::R | MapPermission::W,
            );
        // 在应用的内核栈顶压入一个跳转到 trap_return 而不是 __restore 的任务上下文使得可以第一次 执行该应用
        let task_cx_ptr = (kernel_stack_top - core::mem::size_of::<TaskContext>()) as *mut TaskContext;
        unsafe { *task_cx_ptr = TaskContext::goto_trap_return(); }
        // 开始我们用上面的信息来创建任务控制块实例 task_control_block
        let task_control_block = Self {
            task_cx_ptr: task_cx_ptr as usize,
            task_status,
            task_stride: 0,
            task_priority: TASK_INIT_PRIORITY,
            task_run_duration_ms: 0,
            task_last_start_time: 0,
            memory_set,
            trap_cx_ppn,
            base_size: user_sp,
        };
        // 我们需要初始化该应用的 Trap 上下文，由于它是在应用地址空间而不是在内核地址空间中
        // 我们只能手动查页表找到 Trap 上下文实际被放在的物理页帧，
        // 然后通过之前介绍的 在内核地址空间读写特定物理页帧的能力 获得在用户空间的 Trap 上下文的可变引用用于初始化
        // prepare TrapContext in user space
        let trap_cx = task_control_block.get_trap_cx();
        *trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.lock().token(),
            kernel_stack_top,
            trap_handler as usize,
        );
        task_control_block
    }
}

// 未初始化、准备执行、正在执行、已退出
#[derive(Copy, Clone, PartialEq)] // 让编译器为你的类型提供一些 Trait 的默认实现
pub enum TaskStatus {
    Ready, // a.k.a Runnable
    Running,
    Exited,
}
