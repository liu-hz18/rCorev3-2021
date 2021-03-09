// 操作系统的核心机制 —— 任务切换

mod context;
mod switch;
mod task;

use crate::config::{MAX_APP_NUM, TASK_INIT_PRIORITY, MAX_EXECUTE_TIME_MS};
use crate::loader::{get_num_app, init_app_cx};
use core::cell::RefCell;
use lazy_static::*;
use switch::__switch;
use task::{TaskControlBlock, TaskStatus};
use crate::timer::{get_time_ms};

pub use context::TaskContext;

// 变量与常量分离
pub struct TaskManager {
    num_app: usize, // 任务管理器管理的应用的数目, 在 TaskManager 初始化之后就不会发生变化
    inner: RefCell<TaskManagerInner>,
}

struct TaskManagerInner {
    tasks: [TaskControlBlock; MAX_APP_NUM],
    current_task: usize,
}

unsafe impl Sync for TaskManager {}

lazy_static! {
    pub static ref TASK_MANAGER: TaskManager = {
        let num_app = get_num_app();
        // 创建一个初始化的 tasks 数组，其中的每个任务控制块的运行状态都是 UnInit 代表尚未初始化
        let mut tasks = [
            TaskControlBlock {
                task_cx_ptr: 0,
                task_status: TaskStatus::UnInit,
                task_stride: 0,
                task_priority: TASK_INIT_PRIORITY,
                task_run_duration_ms: 0,
                task_last_start_time: 0,
            };
            MAX_APP_NUM
        ];
        // 依次对每个任务控制块进行初始化
        for i in 0..num_app {
            // 在它的内核栈栈顶压入一些初始化 的上下文
            // 保证应用在第一次执行的时候，内核栈就有 Task 上下文
            // 然后更新它的 task_cx_ptr
            tasks[i].task_cx_ptr = init_app_cx(i) as * const _ as usize;
            tasks[i].task_status = TaskStatus::Ready; // 运行状态设置为 Ready
        }
        TaskManager {
            num_app,
            inner: RefCell::new(TaskManagerInner {
                tasks,
                current_task: 0,
            }),
        }
    };
}

impl TaskManager {
    fn current_task_id(&self) -> usize {
        self.inner.borrow_mut().current_task
    }

    fn set_task_priority(&self, priority: isize) {
        let mut inner = self.inner.borrow_mut();
        let current = inner.current_task;
        inner.tasks[current].task_priority = priority;
    }

    fn run_first_task(&self) {
        // 最先执行的编号为 0 的应用的 task_cx_ptr2
        self.inner.borrow_mut().tasks[0].task_status = TaskStatus::Running;
        let next_task_cx_ptr2 = self.inner.borrow().tasks[0].get_task_cx_ptr2();
        let _unused: usize = 0;
        unsafe {
            __switch(
                &_unused as *const _, // 记录当前应用的任务上下文被保存在 哪里, 也就是当前应用内核栈的栈顶
                next_task_cx_ptr2,
            );
            // __switch 前半部分的保存仅仅是在启动栈上保存了一些之后不会用到的数据
            // 自然也无需记录启动栈栈顶的位置。保存一些寄存器之后的 启动栈栈顶的位置将会保存在此变量中
        }
    }

    fn mark_current_suspended(&self) {
        let mut inner = self.inner.borrow_mut();
        let current = inner.current_task;
        if inner.tasks[current].task_run_duration_ms > MAX_EXECUTE_TIME_MS {
            inner.tasks[current].task_status = TaskStatus::Exited;
            println!("[kernel] Application {} killed by core due to execution duration > {}s.", current, MAX_EXECUTE_TIME_MS / 1000);
        } else {
            inner.tasks[current].task_status = TaskStatus::Ready;
        }
    }

    fn mark_current_exited(&self) {
        let mut inner = self.inner.borrow_mut();
        let current = inner.current_task;
        inner.tasks[current].task_status = TaskStatus::Exited;
    }

    // 实际上实现了 时间片轮转算法 Round-Robin (RR), 也就是 循环队列
    #[allow(dead_code)]
    fn find_next_task(&self) -> Option<usize> {
        let inner = self.inner.borrow();
        let current = inner.current_task;
        // [current + 1, current + self.num_app + 1) % self.num_app, O(n)
        (current + 1..current + self.num_app + 1)
            .map(|id| id % self.num_app)
            .find(|id| {
                inner.tasks[*id].task_status == TaskStatus::Ready
            })
        
    }

    // 实现带优先级的调度算法: stride 调度算法
    fn find_next_task_stride(&self) -> Option<usize> {
        let inner = self.inner.borrow();
        let current = inner.current_task;
        // 循环一圈，从当前 Ready 态的进程中选择 stride 最小的进程调度
        let mut min_task_id: Option<usize> = None;
        let mut min_task_stride: isize = isize::MAX;
        // [current + 1, current + self.num_app + 1) % self.num_app, O(n)
        for id in (current + 1)..(current + self.num_app + 1) {
            if inner.tasks[id % self.num_app].task_status == TaskStatus::Ready && inner.tasks[id % self.num_app].task_stride < min_task_stride {
                min_task_id = Some(id % self.num_app);
                min_task_stride = inner.tasks[id % self.num_app].task_stride
            }
        }
        min_task_id
    }

    fn run_next_task(&self) {
        // 寻找一个运行状态为 Ready 的应用并返回其 ID, 返回的类型是 Option<usize>
        if let Some(next) = self.find_next_task_stride() {
            let mut inner = self.inner.borrow_mut();
            let current = inner.current_task;
            inner.tasks[next].task_status = TaskStatus::Running;
            inner.tasks[next].task_stride += inner.tasks[next].get_task_pass();
            inner.current_task = next;
            // 拿到当前应用 current 和即将被切换到的应用 next 的 task_cx_ptr2 
            let current_task_cx_ptr2 = inner.tasks[current].get_task_cx_ptr2();
            let next_task_cx_ptr2 = inner.tasks[next].get_task_cx_ptr2();
            // 一般情况下它是在 函数退出之后才会被自动释放
            // 从而 TASK_MANAGER 的 inner 字段得以回归到未被借用的状态，之后可以再 借用
            // 如果不手动 drop 的话，编译器会在 __switch 返回，也就是当前应用被切换回来的时候才 drop，这期间我们 都不能修改 TaskManagerInner ，甚至不能读（因为之前是可变借用）
            inner.tasks[current].task_run_duration_ms += get_time_ms() - inner.tasks[current].task_last_start_time;
            inner.tasks[next].task_last_start_time = get_time_ms();
            // println!("[kernel] switch out task {}, time-elasped {}", current, inner.tasks[current].task_run_duration_ms);
            core::mem::drop(inner);
            // 调用 __switch 接口进行切换
            unsafe {
                __switch(
                    current_task_cx_ptr2,
                    next_task_cx_ptr2,
                );
            }
        } else {
            panic!("All applications completed!");
        }
    }
}

pub fn run_first_task() {
    TASK_MANAGER.run_first_task();
}

fn run_next_task() {
    TASK_MANAGER.run_next_task();
}

fn mark_current_suspended() {
    TASK_MANAGER.mark_current_suspended();
}

fn mark_current_exited() {
    TASK_MANAGER.mark_current_exited();
}

pub fn suspend_current_and_run_next() {
    // 先修改当前应用的运行状态
    mark_current_suspended();
    // 尝试切换到下一个应用。
    run_next_task();
}

pub fn exit_current_and_run_next() {
    mark_current_exited();
    run_next_task();
}

pub fn current_task_id() -> usize {
    TASK_MANAGER.current_task_id()
}

pub fn set_task_priority(priority: isize) -> isize {
    if priority >= 2 && priority <= isize::MAX {
        TASK_MANAGER.set_task_priority(priority);
        priority
    } else {
        -1
    }
}
