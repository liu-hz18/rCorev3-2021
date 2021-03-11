use crate::task::{
    suspend_current_and_run_next,
    exit_current_and_run_next,
    current_task_id,
    set_task_priority,
    map_virtual_block,
    unmap_virtual_block,
};
use crate::timer::{get_time_sys, TimeVal};

// 打印退出的应用程序的返回值并同样调用 run_next_app 切换到下一个应用程序
pub fn sys_exit(exit_code: i32) -> ! {
    // 在退出之前我们打印应用的退出信息并输出它的退出码。
    println!("[kernel] Application {} exited with code {}", current_task_id(), exit_code);
    exit_current_and_run_next(); // 退出当前的应用并切换到下个应用
    panic!("Unreachable in sys_exit!");
}

/// 功能：应用主动交出 CPU 所有权并切换到其他应用。
/// 返回值：总是返回 0。
/// syscall ID：124
pub fn sys_yield() -> isize {
    suspend_current_and_run_next(); // 暂停当前的应用并切换到下个应用
    0
}

pub fn sys_get_time(ts: *mut TimeVal, tz: usize) -> isize {
    get_time_sys(ts, tz) as isize
}

pub fn sys_set_priority(priority: isize) -> isize {
    set_task_priority(priority)
}

// 申请长度为 len 字节的物理内存
// 并映射到 addr 开始的虚存，内存页属性为 port
// addr 要求按页对齐(否则报错)，len 可直接按页上取整
// 不考虑分配失败时的页回收（也就是内存泄漏）
pub fn sys_mmap(
    start: usize, // 需要映射的虚存起始地址
    len: usize, // 映射字节长度，可以为 0 （如果是则直接返回），不可过大(上限 1GiB )
    port: usize // 第 0 位表示是否可读，第 1 位表示是否可写，第 2 位表示是否可执行。其他位无效（必须为 0 ）
) -> isize { // 正确时返回实际 map size（为 4096 的倍数），错误返回 -1
    // 失败的情况
    // 1. [addr, addr + len) 存在已经被映射的页
    // 2. 物理内存不足
    // 3. port & !0x7 != 0 (port 其余位必须为0)
    // 4. port & 0x7 = 0 (这样的内存无意义)
    // rust按 字节取反 应该使用 `!`
    map_virtual_block(start, len, port)
}

// 取消一块虚存的映射
pub fn sys_munmap(
    start: usize,
    len: usize,
) -> isize {
    // 参数错误时不考虑内存的恢复和回收
    // 失败的情况:
    // 1. [start, start + len) 中存在未被映射的虚存
    unmap_virtual_block(start, len)
}
