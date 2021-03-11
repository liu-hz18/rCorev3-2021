use super::TaskControlBlock;
use alloc::collections::{VecDeque, BinaryHeap};
use alloc::sync::Arc;
use spin::Mutex;
use lazy_static::*;
use core::cmp::Reverse;

pub struct TaskManager {
    ready_queue: VecDeque<Arc<TaskControlBlock>>,
}

/// A simple FIFO scheduler.
impl TaskManager {
    pub fn new() -> Self {
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

// TODO: Stride Algo. TaskManager using alloc::collections::binary_heap::BinaryHeap
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
    pub static ref TASK_MANAGER: Mutex<TaskManager> = Mutex::new(TaskManager::new());
}

pub fn add_task(task: Arc<TaskControlBlock>) {
    TASK_MANAGER.lock().add(task);
}

pub fn fetch_task() -> Option<Arc<TaskControlBlock>> {
    TASK_MANAGER.lock().fetch()
}
