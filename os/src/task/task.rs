// 把应用程序的一个计算阶段的执行过程（也是一段执行流）称为一个 任务
use crate::mm::{
    MemorySet,
    PhysPageNum,
    KERNEL_SPACE,
    VirtAddr,
    translated_refmut
};
use crate::trap::{TrapContext, trap_handler};
use crate::config::{BIG_STRIDE, TASK_INIT_PRIORITY, TRAP_CONTEXT};
use super::TaskContext;
use super::{PidHandle, pid_alloc, KernelStack};
use alloc::sync::{Weak, Arc};
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use spin::{Mutex, MutexGuard};
use core::cmp::{Ordering};
use crate::fs::{File, Stdin, Stdout, MailBox};

// 进程控制块
// 线程成为CPU（也称处理器）调度（scheduling）和分派（switch）的对象
// 每个 进程 有各自独立的一块内存，使得各个进程之间内存地址相互隔离
// 各个 线程 之间共享进程的地址空间，但 线程有自己独立的栈 。且线程是处理器调度和分派的基本单位
// 协程: 是由用户态的协程管理库来进行管理和调度，操作系统是看不到协程的
// 协程的整个处理过程不需要有特权级切换和操作系统的直接介入
pub struct TaskControlBlock {
    // immutable
    pub pid: PidHandle,
    pub kernel_stack: KernelStack,
    // mutable
    inner: Mutex<TaskControlBlockInner>,
}

// 管理程序的执行过程的任务上下文，控制程序的执行与暂停
pub struct TaskControlBlockInner {
    pub task_cx_ptr: usize, // 一个暂停的任务的任务上下文在内核地址空间（更确切的说是在自身内核栈）中的位置，用于任务切换
    pub task_status: TaskStatus,

    pub task_stride: isize,
    pub task_priority: isize,
    
    pub memory_set: MemorySet, // 应用的地址空间 
    pub trap_cx_ppn: PhysPageNum, // 位于应用地址空间次高页的 Trap 上下文被实际存放在物理页帧的物理页号
    pub base_size: usize, // 应用数据的大小，也就是 在应用地址空间中从 0x0 开始到用户栈结束一共包含多少字节

    pub parent: Option<Weak<TaskControlBlock>>, // 使用 Weak 而非 Arc 来包裹另一个任务控制块，因此这个智能指针将不会影响父进程的引用计数
    pub children: Vec<Arc<TaskControlBlock>>,
    pub exit_code: i32,

    pub fd_table: Vec<Option<Arc<dyn File + Send + Sync>>>, // 文件描述符表
    // Vec 的动态长度特性使得我们无需设置一个固定的文件描述符数量上限
    // Option 使得我们可以区分一个文件描述符当前是否空闲，当它是 None 的时候是空闲的，而 Some 则代表它已被占用
    // Arc 首先提供了共享引用能力, 可能会有多个进程共享同一个文件对它进行读写
    // dyn 关键字表明 Arc 里面的类型实现了 File/Send/Sync 三个 Trait, 等到运行时才能知道它的具体类型 (Rust 多态)
    pub mail_box: MailBox,
}
// 子进程的进程控制块并不会被直接放到父进程控制块下面，因为子进程完全有可能在父进程退出后仍然存在
// 因此进程控制块的本体是被放到内核堆上面的，对于它的一切访问都是通过智能指针 Arc/Weak 来进行的
// 当且仅当它的引用计数变为 0 的时候，进程控制块以及被绑定到它上面的各类资源才会被回收

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
    pub fn get_user_token(&self) -> usize {
        self.memory_set.token()
    }
    fn get_status(&self) -> TaskStatus {
        self.task_status
    }
    pub fn is_zombie(&self) -> bool {
        self.get_status() == TaskStatus::Zombie
    }
    // 最先匹配
    // 在进程控制块中分配一个最小的空闲文件描述符来访问一个新打开的文件
    pub fn alloc_fd(&mut self) -> usize {
        // 从小到大遍历所有曾经被分配过的文件描述符尝试找到一个空闲的
        if let Some(fd) = (0..self.fd_table.len())
            .find(|fd| self.fd_table[*fd].is_none()) {
            fd
        } else { // 如果没有的话就需要拓展文件描述符表的长度并新分配一个
            self.fd_table.push(None); // 一开始是None, 因为这时候只是分配了描述符，还不知道是什么文件
            self.fd_table.len() - 1
        }
    }
}

impl TaskControlBlock {
    pub fn acquire_inner_lock(&self) -> MutexGuard<TaskControlBlockInner> {
        self.inner.lock()
    }
    // 创建一个新的进程，目前仅用于内核中手动创建唯一一个初始进程 initproc
    pub fn new(elf_data: &[u8]) -> Self {
        // memory_set with elf program headers/trampoline/trap context/user stack
        // 解析传入的 ELF 格式数据构造应用的地址空间 memory_set 并获得其他信息
        // 用户栈在应用地址空间中的位置 user_sp 以及应用的入口点 entry_point
        let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
        // 地址空间 memory_set 中查多级页表找到应用地址空间中的 Trap 上下文实际被放在哪个物理页帧
        // 手动查页表找到应用地址空间中的 Trap 上下文被实际放在哪个物理页帧上，用来做后续的初始化
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
        // 记录下内核栈在内核地址空间的位置 kernel_stack_top
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

                memory_set,
                trap_cx_ppn,
                base_size: user_sp,

                parent: None,
                children: Vec::new(),
                exit_code: 0,
                // 内核会默认为其打开三个文件
                fd_table: vec![
                    // 0 -> stdin
                    Some(Arc::new(Stdin)), // 文件描述符为 0 的标准输入
                    // 1 -> stdout
                    Some(Arc::new(Stdout)), // 文件描述符为 1 的标准输出；
                    // 2 -> stderr
                    Some(Arc::new(Stdout)), // 文件描述符为 2 的标准错误输出
                ],
                mail_box: MailBox::new(),
                // 在我们的实现中并不区分标准输出和标准错误输出
                // 进程打开一个文件的时候，内核总是会将文件分配到该进程文件描述符表中 最小的 空闲位置 (最先匹配算法)
            }),
        };
        // prepare TrapContext in user space
        // 我们需要初始化该应用的 Trap 上下文，由于它是在应用地址空间而不是在内核地址空间中
        // 我们只能手动查页表找到 Trap 上下文实际被放在的物理页帧，
        // 然后通过之前介绍的 在内核地址空间读写特定物理页帧的能力 获得在用户空间的 Trap 上下文的可变引用用于初始化
        // 使得第一次进入用户态的时候时候能正确跳转到应用入口点并设置好用户栈，同时也保证在 Trap 的时候用户态能正确进入内核态
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
    // 用来实现 exec 系统调用，即当前进程加载并执行另一个 ELF 格式可执行文件
    pub fn exec(&self, elf_data: &[u8], args: Vec<String>) {
        // memory_set with elf program headers/trampoline/trap context/user stack
        let (memory_set, mut user_sp, entry_point) = MemorySet::from_elf(elf_data);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT).into())
            .unwrap()
            .ppn();
        // push arguments on user stack
        user_sp -= (args.len() + 1) * core::mem::size_of::<usize>();
        let argv_base = user_sp;
        let mut argv: Vec<_> = (0..=args.len())
            .map(|arg| {
                translated_refmut(
                    memory_set.token(),
                    (argv_base + arg * core::mem::size_of::<usize>()) as *mut usize
                )
            })
            .collect();
        *argv[args.len()] = 0;
        for i in 0..args.len() {
            user_sp -= args[i].len() + 1;
            *argv[i] = user_sp;
            let mut p = user_sp;
            for c in args[i].as_bytes() {
                *translated_refmut(memory_set.token(), p as *mut u8) = *c;
                p += 1;
            }
            *translated_refmut(memory_set.token(), p as *mut u8) = 0;
        }
        // **** hold current PCB lock
        let mut inner = self.acquire_inner_lock();
        // substitute memory_set
        // 从 ELF 生成一个全新的地址空间并直接替换进来
        // 这将导致原有的地址空间生命周期结束，里面包含的全部物理页帧都会被回收
        inner.memory_set = memory_set;
        // update trap_cx ppn
        inner.trap_cx_ppn = trap_cx_ppn;
        // initialize trap_cx
        // 修改新的地址空间中的 Trap 上下文，将解析得到的应用入口点、用户栈位置以及一些内核的信息进行初始化，这样才能正常实现 Trap 机制
        let mut trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.lock().token(),
            self.kernel_stack.get_top(),
            trap_handler as usize,
        );
        trap_cx.x[10] = args.len();
        trap_cx.x[11] = argv_base;
        *inner.get_trap_cx() = trap_cx;
        // **** release current PCB lock
    }
    // 实现 fork 系统调用，即当前进程 fork 出来一个与之几乎相同的子进程
    pub fn fork(self: &Arc<TaskControlBlock>) -> Arc<TaskControlBlock> {
        // ---- hold parent PCB lock
        let mut parent_inner = self.acquire_inner_lock();
        // 复制父进程地址空间
        // 两个进程的应用数据由于地址空间复制的原因也是完全相同的
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
        // 子进程内核栈上压入一个初始化的任务上下文，使得内核一旦通过任务切换到该进程，就会跳转到 trap_return 来进入用户态
        let task_cx_ptr = kernel_stack.push_on_top(TaskContext::goto_trap_return());
        // copy fd table, 子进程需要完全继承父进程的文件描述符表来和父进程共享所有文件
        // 这样，即使我们 仅手动为初始进程 initproc 打开了标准输入输出，所有进程也都可以访问它们
        let mut new_fd_table: Vec<Option<Arc<dyn File + Send + Sync>>> = Vec::new();
        for fd in parent_inner.fd_table.iter() {
            if let Some(file) = fd {
                new_fd_table.push(Some(file.clone()));
            } else {
                new_fd_table.push(None);
            }
        }
        let mut new_mail_box = MailBox::new();
        for mail in parent_inner.mail_box.packets.iter() {
            new_mail_box.push(*mail);
        }
        let task_control_block = Arc::new(TaskControlBlock {
            pid: pid_handle,
            kernel_stack,
            inner: Mutex::new(TaskControlBlockInner {
                task_cx_ptr: task_cx_ptr as usize,
                task_status: TaskStatus::Ready,

                task_stride: 0,
                task_priority: parent_inner.task_priority,

                memory_set,
                trap_cx_ppn,
                base_size: parent_inner.base_size, // 让子进程和父进程的 base_size ，也即应用数据的大小保持一致

                parent: Some(Arc::downgrade(self)), // 将父进程的弱引用计数放到子进程的进程控制块中
                children: Vec::new(),
                exit_code: 0,

                fd_table: new_fd_table,

                mail_box: new_mail_box,
            }),
        });
        // 注意父子进程关系的维护
        // add child
        parent_inner.children.push(task_control_block.clone());
        drop(parent_inner);
        // modify kernel_sp in trap_cx
        // 子进程的 Trap 上下文也是完全从父进程复制过来的
        // 保证子进程进入用户态和其父进程回到用户态的那一瞬间 CPU 的状态是完全相同的
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
