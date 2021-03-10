use crate::task::{
    suspend_current_and_run_next,
    exit_current_and_run_next,
    current_task_id,
    set_task_priority,
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
