use super::TaskControlBlock;
use alloc::collections::{VecDeque, BinaryHeap};
use alloc::sync::Arc;
use spin::Mutex;
use lazy_static::*;
use core::cmp::Reverse;

// 任务管理器
// 这里，任务指的就是进程
pub struct TaskManager {
    // 在任务管理器中仅存放他们的引用计数智能指针
    // 这样做的原因在于，任务控制块经常需要被放入/取出，如果直接移动任务控制块自身将会带来大量的数据拷贝开销
    ready_queue: VecDeque<Arc<TaskControlBlock>>,
}

/// A simple FIFO scheduler.
impl TaskManager {
    pub fn new() -> Self {
        // 双端队列
        Self { ready_queue: VecDeque::new(), }
    }
    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        self.ready_queue.push_back(task);
    }
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        self.ready_queue.pop_front()
    }
    pub fn running_num(&self) -> usize {
        self.ready_queue.len()
    }
}

// Stride Algo. TaskManager using alloc::collections::binary_heap::BinaryHeap
pub struct StrideTaskManager {
    ready_queue: BinaryHeap<Reverse<Arc<TaskControlBlock>>>
}

impl StrideTaskManager {
    pub fn new() -> Self {
        Self { ready_queue: BinaryHeap::new(), }
    }
    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        self.ready_queue.push(Reverse(task));
    }
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        if let Some(Reverse(task)) = self.ready_queue.pop() {
            Some(task)
        } else {
            None
        }
    }
    pub fn running_num(&self) -> usize {
        self.ready_queue.len()
    }
}

lazy_static! {
    // pub static ref TASK_MANAGER: Mutex<StrideTaskManager> = Mutex::new(StrideTaskManager::new());
    pub static ref TASK_MANAGER: Mutex<TaskManager> = Mutex::new(TaskManager::new());
}

pub fn add_task(task: Arc<TaskControlBlock>) {
    TASK_MANAGER.lock().add(task);
}

pub fn fetch_task() -> Option<Arc<TaskControlBlock>> {
    TASK_MANAGER.lock().fetch()
}

pub fn running_task_num() -> usize {
    TASK_MANAGER.lock().running_num()
}
