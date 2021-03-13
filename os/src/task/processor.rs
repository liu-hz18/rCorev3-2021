use super::TaskControlBlock;
use alloc::sync::Arc;
use core::cell::RefCell;
use lazy_static::*;
use super::{fetch_task, TaskStatus};
use super::__switch;
use crate::trap::TrapContext;
use crate::config::{BIG_STRIDE};

// 处理器监视器
// 处理器监视器 Processor 负责从任务管理器 TaskManager 分离出去的那部分维护 CPU 状态的职责：
//  是一种 per-CPU 的数据结构，即每个核都有一份专属的 Processor 结构体，只有这个核自己会访问它
pub struct Processor {
    inner: RefCell<ProcessorInner>,
}

// 因此无论是单核还是多核环境，在访问 Processor 的时候都不会带来任何隐含的数据竞争风险，
// 这样我们就可以将 Processor 标记为 Sync 并全局实例化
unsafe impl Sync for Processor {}

struct ProcessorInner {
    // 当前处理器上正在执行的任务
    current: Option<Arc<TaskControlBlock>>,
    // 它是目前在内核中以硬编码方式创建的唯一一个进程
    // 他所有的进程都是通过一个名为 fork 的系统调用来创建的
    idle_task_cx_ptr: usize, // 当前处理器上的 idle 执行流的任务上下文的地址
}

impl Processor {
    pub fn new() -> Self {
        Self {
            inner: RefCell::new(ProcessorInner {
                current: None,
                idle_task_cx_ptr: 0, // 在内核初始化完毕之后会创建一个进程——即 初始进程 (Initial Process)
            }),
        }
    }
    // idle 执行流
    // 它们运行在每个核各自的启动栈上，功能是尝试从任务管理器中选出一个任务来在当前核上执行
    // 在内核初始化完毕之后，每个核都会通过调用 run_tasks 函数来进入 idle 执行流
    fn get_idle_task_cx_ptr2(&self) -> *const usize {
        let inner = self.inner.borrow();
        &inner.idle_task_cx_ptr as *const usize
    }
    pub fn run(&self) {
        loop {
            if let Some(task) = fetch_task() {
                let idle_task_cx_ptr2 = self.get_idle_task_cx_ptr2();
                // acquire
                let mut task_inner = task.acquire_inner_lock();
                let next_task_cx_ptr2 = task_inner.get_task_cx_ptr2();
                task_inner.task_status = TaskStatus::Running;
                task_inner.task_stride += BIG_STRIDE / task_inner.task_priority;
                drop(task_inner);
                // release
                // Arc<TaskControlBlock> 形式的任务从任务管理器流动到了处理器监视器中
                // 也就是说，在稳定的情况下，每个尚未结束的进程的任务控制块都只能被引用一次，要么在任务管理器中，要么则是在某个处理器的 Processor 中
                self.inner.borrow_mut().current = Some(task);
                // 从当前的 idle 执行流切换到接下来要执行的任务
                unsafe {
                    __switch(
                        idle_task_cx_ptr2,
                        next_task_cx_ptr2,
                    );
                }
            } else {
                panic!("[kernel] No more tasks. Shutting Down!");
            }
        }
    }
    // 取出 当前正在执行的任务
    pub fn take_current(&self) -> Option<Arc<TaskControlBlock>> {
        self.inner.borrow_mut().current.take()
    }
    // 返回当前执行的任务的一份拷贝
    pub fn current(&self) -> Option<Arc<TaskControlBlock>> {
        self.inner.borrow().current.as_ref().map(|task| Arc::clone(task))
    }
}

lazy_static! {
    pub static ref PROCESSOR: Processor = Processor::new();
}

pub fn run_tasks() {
    PROCESSOR.run();
}

// 注意: 这个函数和 current_task() 不同，这个函数会清除 PROCESSOR 里的 TaskControlBlock
pub fn take_current_task() -> Option<Arc<TaskControlBlock>> {
    PROCESSOR.take_current()
}

pub fn current_task_id() -> usize {
    PROCESSOR.current().unwrap().getpid()
}

pub fn set_task_priority(priority: isize) -> isize {
    PROCESSOR.current().unwrap().set_priority(priority)
}

pub fn current_task() -> Option<Arc<TaskControlBlock>> {
    PROCESSOR.current()
}

pub fn current_user_token() -> usize {
    let task = current_task().unwrap();
    let token = task.acquire_inner_lock().get_user_token();
    token
}

pub fn current_trap_cx() -> &'static mut TrapContext {
    current_task().unwrap().acquire_inner_lock().get_trap_cx()
}

pub fn schedule(switched_task_cx_ptr2: *const usize) {
    // 切换到 idle 执行流并开启新一轮的任务调度
    // 我们将跳转到 Processor::run 中 __switch 返回之后的位置，也即开启了下一轮循环
    let idle_task_cx_ptr2 = PROCESSOR.get_idle_task_cx_ptr2();
    unsafe {
        __switch(
            switched_task_cx_ptr2,
            idle_task_cx_ptr2,
        );
    }
}
