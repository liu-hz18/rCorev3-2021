// 把应用程序的一个计算阶段的执行过程（也是一段执行流）称为一个 任务
use crate::mm::{MemorySet, PhysPageNum, KERNEL_SPACE, VirtAddr};
use crate::trap::{TrapContext, trap_handler};
use crate::config::{BIG_STRIDE, TASK_INIT_PRIORITY, TRAP_CONTEXT};
use super::TaskContext;
use super::{PidHandle, pid_alloc, KernelStack};
use alloc::sync::{Weak, Arc};
use alloc::vec::Vec;
use spin::{Mutex, MutexGuard};
use core::cmp::{Ordering};

#[derive(Debug)]
pub struct TaskControlBlock {
    // immutable
    pub pid: PidHandle,
    pub kernel_stack: KernelStack,
    // mutable
    inner: Mutex<TaskControlBlockInner>,
}

// 管理程序的执行过程的任务上下文，控制程序的执行与暂停
#[derive(Debug)]
pub struct TaskControlBlockInner {
    pub task_cx_ptr: usize,
    pub task_status: TaskStatus,

    pub task_stride: isize,
    pub task_priority: isize,
    pub task_run_duration_ms: usize,
    pub task_last_start_time: usize,
    
    pub memory_set: MemorySet, // 应用的地址空间 
    pub trap_cx_ppn: PhysPageNum, // 位于应用地址空间次高页的 Trap 上下文被实际存放在物理页帧的物理页号
    pub base_size: usize, // 应用数据的大小，也就是 在应用地址空间中从 0x0 开始到用户栈结束一共包含多少字节

    pub parent: Option<Weak<TaskControlBlock>>,
    pub children: Vec<Arc<TaskControlBlock>>,
    pub exit_code: i32,
}

impl PartialEq for TaskControlBlock {
    fn eq(&self, other: &Self) -> bool {
        self.acquire_inner_lock().task_stride == other.acquire_inner_lock().task_stride
    }
}

impl PartialOrd for TaskControlBlock {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.acquire_inner_lock().task_stride.partial_cmp(&(other.acquire_inner_lock().task_stride))
    }
}

impl Eq for TaskControlBlock {}

impl Ord for TaskControlBlock {
    fn cmp(&self, other: &Self) -> Ordering {
        self.acquire_inner_lock().task_stride.cmp(&(other.acquire_inner_lock().task_stride))
    }
}

impl TaskControlBlockInner {
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
    fn get_status(&self) -> TaskStatus {
        self.task_status
    }
    pub fn is_zombie(&self) -> bool {
        self.get_status() == TaskStatus::Zombie
    }
    
}

impl TaskControlBlock {
    pub fn acquire_inner_lock(&self) -> MutexGuard<TaskControlBlockInner> {
        self.inner.lock()
    }
    pub fn new(elf_data: &[u8]) -> Self {
        // memory_set with elf program headers/trampoline/trap context/user stack
        // 解析传入的 ELF 格式数据构造应用的地址空间 memory_set 并获得其他信息
        let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
        // 地址空间 memory_set 中查多级页表找到应用地址空间中的 Trap 上下文实际被放在哪个物理页帧
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT).into())
            .unwrap()
            .ppn();
        // alloc a pid and a kernel stack in kernel space
        let pid_handle = pid_alloc();
        // map a kernel-stack in kernel space
        // 我们根据传入的应用 ID app_id 调用在 config 子模块中定义的 kernel_stack_position 找到 应用的内核栈预计放在内核地址空间 KERNEL_SPACE 中的哪个位置，
        // 并通过 insert_framed_area 实际将这个逻辑段 加入到内核地址空间中
        let kernel_stack = KernelStack::new(&pid_handle);
        let kernel_stack_top = kernel_stack.get_top();
        // push a task context which goes to trap_return to the top of kernel stack
        // 在应用的内核栈顶压入一个跳转到 trap_return 而不是 __restore 的任务上下文使得可以第一次 执行该应用
        let task_cx_ptr = kernel_stack.push_on_top(TaskContext::goto_trap_return());
        // 开始我们用上面的信息来创建任务控制块实例 task_control_block
        let task_control_block = Self {
            pid: pid_handle,
            kernel_stack,
            inner: Mutex::new(TaskControlBlockInner {
                task_cx_ptr: task_cx_ptr as usize,
                task_status: TaskStatus::Ready,

                task_stride: 0,
                task_priority: TASK_INIT_PRIORITY,
                task_run_duration_ms: 0,
                task_last_start_time: 0,

                memory_set,
                trap_cx_ppn,
                base_size: user_sp,

                parent: None,
                children: Vec::new(),
                exit_code: 0,
            }),
        };
        // prepare TrapContext in user space
        // 我们需要初始化该应用的 Trap 上下文，由于它是在应用地址空间而不是在内核地址空间中
        // 我们只能手动查页表找到 Trap 上下文实际被放在的物理页帧，
        // 然后通过之前介绍的 在内核地址空间读写特定物理页帧的能力 获得在用户空间的 Trap 上下文的可变引用用于初始化
        // prepare TrapContext in user space
        let trap_cx = task_control_block.acquire_inner_lock().get_trap_cx();
        *trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.lock().token(),
            kernel_stack_top,
            trap_handler as usize,
        );
        task_control_block
    }
    pub fn exec(&self, elf_data: &[u8]) {
        // memory_set with elf program headers/trampoline/trap context/user stack
        let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT).into())
            .unwrap()
            .ppn();

        // **** hold current PCB lock
        let mut inner = self.acquire_inner_lock();
        // substitute memory_set
        inner.memory_set = memory_set;
        // update trap_cx ppn
        inner.trap_cx_ppn = trap_cx_ppn;
        // initialize trap_cx
        let trap_cx = inner.get_trap_cx();
        *trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.lock().token(),
            self.kernel_stack.get_top(),
            trap_handler as usize,
        );
        // **** release current PCB lock
    }
    pub fn fork(self: &Arc<TaskControlBlock>) -> Arc<TaskControlBlock> {
        // ---- hold parent PCB lock
        let mut parent_inner = self.acquire_inner_lock();
        // copy user space(include trap context)
        let memory_set = MemorySet::from_existed_user(
            &parent_inner.memory_set
        );
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT).into())
            .unwrap()
            .ppn();
        // alloc a pid and a kernel stack in kernel space
        let pid_handle = pid_alloc();
        let kernel_stack = KernelStack::new(&pid_handle);
        let kernel_stack_top = kernel_stack.get_top();
        // push a goto_trap_return task_cx on the top of kernel stack
        let task_cx_ptr = kernel_stack.push_on_top(TaskContext::goto_trap_return());
        let task_control_block = Arc::new(TaskControlBlock {
            pid: pid_handle,
            kernel_stack,
            inner: Mutex::new(TaskControlBlockInner {
                task_cx_ptr: task_cx_ptr as usize,
                task_status: TaskStatus::Ready,

                task_stride: 0,
                task_priority: parent_inner.task_priority,
                task_run_duration_ms: 0,
                task_last_start_time: 0,

                memory_set,
                trap_cx_ppn,
                base_size: parent_inner.base_size,

                parent: Some(Arc::downgrade(self)),
                children: Vec::new(),
                exit_code: 0,
            }),
        });
        // add child
        parent_inner.children.push(task_control_block.clone());
        // modify kernel_sp in trap_cx
        // **** acquire child PCB lock
        let trap_cx = task_control_block.acquire_inner_lock().get_trap_cx();
        // **** release child PCB lock
        trap_cx.kernel_sp = kernel_stack_top;
        // return
        task_control_block
        // ---- release parent PCB lock
    }
    pub fn getpid(&self) -> usize {
        self.pid.0
    }
    pub fn set_priority(&self, priority: isize) -> isize {
        if priority > 1 && priority <= isize::MAX {
            self.acquire_inner_lock().task_priority = priority;
            priority
        } else {
            -1
        }        
    }
}

// 未初始化、准备执行、正在执行、已退出
#[derive(Copy, Clone, PartialEq, Debug)] // 让编译器为你的类型提供一些 Trait 的默认实现
pub enum TaskStatus {
    Ready, // a.k.a Runnable
    Running,
    Zombie,
}
