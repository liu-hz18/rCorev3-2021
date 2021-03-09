// 把应用程序的一个计算阶段的执行过程（也是一段执行流）称为一个 任务
use crate::config::BIG_STRIDE;

// 管理程序的执行过程的任务上下文，控制程序的执行与暂停
pub struct TaskControlBlock {
    pub task_cx_ptr: usize,
    pub task_status: TaskStatus,
    pub task_stride: isize,
    pub task_priority: isize,
    pub task_run_duration_ms: usize,
    pub task_last_start_time: usize,
}

impl TaskControlBlock {
    pub fn get_task_cx_ptr2(&self) -> *const usize {
        &self.task_cx_ptr as *const usize
    }
    pub fn get_task_pass(&self) -> isize {
        BIG_STRIDE / self.task_priority
    }
}

// 未初始化、准备执行、正在执行、已退出
#[derive(Copy, Clone, PartialEq)] // 让编译器为你的类型提供一些 Trait 的默认实现
pub enum TaskStatus {
    UnInit,
    Ready, // a.k.a Runnable
    Running,
    Exited,
}
