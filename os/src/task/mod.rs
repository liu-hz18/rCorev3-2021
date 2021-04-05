mod context;
mod switch;
mod task;
mod manager;
mod processor;
mod pid;

use crate::fs::{open_file, OpenFlags};
use switch::__switch;
use task::{TaskControlBlock, TaskStatus};
use alloc::sync::Arc;
use manager::fetch_task;
use lazy_static::*;
use crate::mm::{MapPermission, MapType, MapArea, VPNRange, VirtAddr, usable_frames};
use crate::config::PAGE_SIZE;

pub use context::TaskContext;
pub use processor::{
    run_tasks,
    current_task,
    current_user_token,
    current_trap_cx,
    take_current_task,
    current_task_id,
    schedule,
    set_task_priority,
};
pub use manager::{add_task, running_task_num, set_task_mail};
pub use pid::{PidHandle, pid_alloc, KernelStack};

// 暂停当前任务并切换到下一个任务
// 注意，当仅有一个任务的时候， suspend_current_and_run_next 的效果是会继续执行这个任务
pub fn suspend_current_and_run_next() {
    // There must be an application running.
    // 取出当前正在执行的任务
    let task = take_current_task().unwrap();

    // ---- hold current PCB lock
    let mut task_inner = task.acquire_inner_lock();
    let task_cx_ptr2 = task_inner.get_task_cx_ptr2();
    // Change status to Ready
    task_inner.task_status = TaskStatus::Ready;
    drop(task_inner);
    // ---- release current PCB lock
    // push back to ready queue.
    add_task(task);
    // jump to scheduling cycle
    schedule(task_cx_ptr2);
}

// 当进程退出的时候内核立即回收一部分资源并将该进程标记为 僵尸进程
pub fn exit_current_and_run_next(exit_code: i32) {
    // take from Processor
    // 将当前进程控制块从处理器监控 PROCESSOR 中取出而不是得到一份拷贝
    // 为了正确维护进程控制块的引用计数
    let task = take_current_task().unwrap();
    // **** hold current PCB lock
    let mut inner = task.acquire_inner_lock();
    // Change status to Zombie
    inner.task_status = TaskStatus::Zombie;
    // Record exit code
    // 将传入的退出码 exit_code 写入进程控制块中，后续父进程在 waitpid 的时候可以收集
    inner.exit_code = exit_code;
    // do not move to its parent but under initproc

    // ++++++ hold initproc PCB lock here
    // 将当前进程的所有子进程挂在初始进程 initproc 下面
    if task.getpid() != INITPROC.getpid() {
        let mut initproc_inner = INITPROC.acquire_inner_lock();
        for child in inner.children.iter() { // 遍历每个子进程
            child.acquire_inner_lock().parent = Some(Arc::downgrade(&INITPROC)); // 修改其父进程为初始进程
            initproc_inner.children.push(child.clone()); // 加入初始进程的孩子向量中
        }
    }
    // ++++++ release parent PCB lock here

    inner.children.clear(); // 将当前进程的孩子向量清空
    // deallocate user space, 对于当前进程占用的资源进行早期回收
    // 只是将地址空间中的逻辑段列表 areas 清空，这将导致应用地址空间的所有数据被存放在的物理页帧被回收，而用来存放页表的那些物理页帧此时则不会被回收
    inner.memory_set.recycle_data_pages();
    drop(inner);
    // **** release current PCB lock
    // drop task manually to maintain rc correctly
    drop(task);
    // we do not have to save task context
    let _unused: usize = 0;
    // println!("unused physical frames: {}", usable_frames());
    // 我们再也不会回到该进程的执行过程中，因此无需关心任务上下文的保存
    schedule(&_unused as *const _);
}

// 将初始进程 initproc 加入任务管理器
lazy_static! {
    pub static ref INITPROC: Arc<TaskControlBlock> = Arc::new({
        let inode = open_file("ch7_usertest", OpenFlags::RDONLY).unwrap();
        let v = inode.read_all();
        TaskControlBlock::new(v.as_slice())
    });
}

pub fn add_initproc() {
    add_task(INITPROC.clone());
}

pub fn map_virtual_pages(addr: usize, len: usize, port: usize) -> isize {
    // addr 按页 (4096 Byte) 对齐, len \in [0, 1GB = 0x4000_0000) 
    // port 其余位必须为0, port & 0x7 = 0
    if addr & (PAGE_SIZE-1) != 0 || len > 0x4000_0000 || (port & !0x7) != 0 || port & 0x7 == 0 { 
        return -1;
    }
    if len == 0 { return 0; }
    let task = current_task().unwrap();
    let mut inner = task.acquire_inner_lock();
    let map_perm = port_to_permission(port);
    let map_area: MapArea = MapArea::new(
        addr.into(),
        (addr+len).into(),
        MapType::Framed,
        map_perm
    );
    let vpn_range: VPNRange = map_area.vpn_range;
    // 处理 虚拟地址区间 [addr, addr+len) 存在已经被映射的页的错误
    for vpn in vpn_range {
        if inner.memory_set.have_mapped(&vpn) {
            return -1;
        }
    }
    let va_start: VirtAddr = vpn_range.get_start().into();
    let va_end: VirtAddr = vpn_range.get_end().into();
    // TODO: 处理物理内存不足的错误, 目前直接panic
    inner.memory_set.push(map_area, None);
    drop(inner);
    (va_end.0 - va_start.0) as isize
}

pub fn unmap_virtual_pages(addr: usize, len: usize) -> isize {
    if addr & (PAGE_SIZE-1) != 0 || len > 0x4000_0000 { 
        return -1;
    }
    if len == 0 { return 0; }
    let task = current_task().unwrap();
    let mut inner = task.acquire_inner_lock();

    let start_va: VirtAddr = addr.into();
    let end_va: VirtAddr = (addr+len).into();
    let vpn_range: VPNRange = VPNRange::new(start_va.floor(), end_va.ceil());
    let va_start: VirtAddr = vpn_range.get_start().into();
    let va_end: VirtAddr = vpn_range.get_end().into();

    // 处理 虚拟地址区间 [addr, addr+len) 存在未被映射的页的错误
    for vpn in vpn_range {
        if !inner.memory_set.have_mapped(&vpn) {
            return -1;
        }
    }
    // unmap 对应的映射
    inner.memory_set.unmap(vpn_range);
    drop(inner);
    (va_end.0 - va_start.0) as isize
}

pub fn port_to_permission(port: usize) -> MapPermission {
    let mut map_perm = MapPermission::U;
    if port & 0x01 != 0 { map_perm |= MapPermission::R; }
    if port & 0x02 != 0 { map_perm |= MapPermission::W; }
    if port & 0x04 != 0 { map_perm |= MapPermission::X; }
    map_perm
}
