use crate::task::{
    suspend_current_and_run_next,
    exit_current_and_run_next,
    current_task_id,
    set_task_priority,
    map_virtual_pages,
    unmap_virtual_pages,
    current_task,
    current_user_token,
    add_task,
};
use crate::timer::{get_time_sys, TimeVal};
use crate::mm::{
    translated_str,
    translated_refmut,
};
use crate::loader::get_app_data_by_name;
use alloc::sync::Arc;

// 打印退出的应用程序的返回值并同样调用 run_next_app 切换到下一个应用程序
pub fn sys_exit(exit_code: i32) -> ! {
    // 在退出之前我们打印应用的退出信息并输出它的退出码。
    // println!("[kernel] Application {} exited with code {}", current_task_id(), exit_code);
    exit_current_and_run_next(exit_code); // 退出当前的应用并切换到下个应用
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
    map_virtual_pages(start, len, port)
}

// 取消一块虚存的映射
pub fn sys_munmap(
    start: usize,
    len: usize,
) -> isize {
    // 参数错误时不考虑内存的恢复和回收
    // 失败的情况:
    // 1. [start, start + len) 中存在未被映射的虚存
    unmap_virtual_pages(start, len)
}

// 返回当前进程的进程 ID。
pub fn sys_getpid() -> isize {
    current_task().unwrap().pid.0 as isize
}

pub fn sys_fork() -> isize {
    let current_task = current_task().unwrap();
    let new_task = current_task.fork();
    let new_pid = new_task.pid.0;
    // modify trap context of new_task, because it returns immediately after switching
    let trap_cx = new_task.acquire_inner_lock().get_trap_cx();
    // we do not have to move to next instruction since we have done it before
    // for child process, fork returns 0
    trap_cx.x[10] = 0;
    // add new task to scheduler
    add_task(new_task);
    new_pid as isize
}

pub fn sys_exec(path: *const u8) -> isize {
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(data) = get_app_data_by_name(path.as_str()) {
        let task = current_task().unwrap();
        task.exec(data);
        0
    } else {
        -1
    }
}

// 当前进程等待一个子进程结束，并获取其返回值
/// If there is not a child process whose pid is same as given, return -1.
/// Else if there is a child process but it is still running, return -2.
// 非阻塞方式，如果存在子进程但是没有执行完，返回-2
pub fn sys_waitpid_non_blocking(
    pid: isize, // 表示要等待结束的子进程的进程 ID, 如果为 0或者-1 的话表示等待任意一个子进程结束
    exit_code_ptr: *mut i32 // 保存子进程返回值的地址，如果这个地址为 0 的话表示不必保存
) -> isize {
    let task = current_task().unwrap();
    // find a child process

    // ---- hold current PCB lock
    let mut inner = task.acquire_inner_lock();
    // 可能的错误:
    //  1. 进程无未结束子进程
    //  2. pid 非法或者指定的不是该进程的子进程。
    //  3. 传入的地址 status 不为 0 但是不合法
    if inner.children
        .iter()
        .find(|p| {pid == -1 || pid == 0 || pid as usize == p.getpid()})
        .is_none() {
        return -1;
        // ---- release current PCB lock
    }
    let pair = inner.children
        .iter()
        .enumerate()
        .find(|(_, p)| {
            // ++++ temporarily hold child PCB lock
            p.acquire_inner_lock().is_zombie() && (pid == -1 || pid == 0 || pid as usize == p.getpid())
            // ++++ release child PCB lock
        });
    if let Some((idx, _)) = pair {
        let child = inner.children.remove(idx);
        // confirm that child will be deallocated after removing from children list
        assert_eq!(Arc::strong_count(&child), 1);
        let found_pid = child.getpid();
        // ++++ temporarily hold child lock
        let exit_code = child.acquire_inner_lock().exit_code;
        // ++++ release child PCB lock
        // 判断 exit_code_ptr 是否合法

        *translated_refmut(inner.memory_set.token(), exit_code_ptr) = exit_code;
        found_pid as isize
    } else {
        -1
    }
    // ---- release current PCB lock automatically
}

pub fn sys_waitpid_blocking(
    pid: isize, // 表示要等待结束的子进程的进程 ID, 如果为 0或者-1 的话表示等待任意一个子进程结束
    exit_code_ptr: *mut i32 // 保存子进程返回值的地址，如果这个地址为 0 的话表示不必保存
) -> isize {
    let task = current_task().unwrap();
    // find a child process
    // ---- hold current PCB lock
    {
        let mut inner = task.acquire_inner_lock();
        // 可能的错误:
        //  1. 进程无未结束子进程
        //  2. pid 非法或者指定的不是该进程的子进程。
        //  3. 传入的地址 status 不为 0 但是不合法
        if inner.children
            .iter()
            .find(|p| {pid == -1 || pid == 0 || pid as usize == p.getpid()})
            .is_none() {
            return -1;
            // ---- release current PCB lock
        }
    }
    loop {
        let mut inner = task.acquire_inner_lock();
        let pair = inner.children
            .iter()
            .enumerate()
            .find(|(_, p)| {
                // ++++ temporarily hold child PCB lock
                p.acquire_inner_lock().is_zombie() && (pid == -1 || pid == 0 || pid as usize == p.getpid())
                // ++++ release child PCB lock
            });
        if let Some((idx, _)) = pair {
            let child = inner.children.remove(idx);
            // confirm that child will be deallocated after removing from children list
            assert_eq!(Arc::strong_count(&child), 1);
            let found_pid = child.getpid();
            // ++++ temporarily hold child lock
            let exit_code = child.acquire_inner_lock().exit_code;
            // ++++ release child PCB lock
            // 判断 exit_code_ptr 是否合法

            *translated_refmut(inner.memory_set.token(), exit_code_ptr) = exit_code;
            drop(inner);
            return found_pid as isize;
        } else {
            // 阻塞方式实现
            drop(inner); // 注意释放互斥锁
            suspend_current_and_run_next();
            continue;
        }
    }
    // ---- release current PCB lock automatically
}

// 创建一个子进程并执行目标路径文件，暂时不考虑参数，不要求立即开始执行，相当于 fork + exec
// 相当于 fork + exec，新建子进程并执行目标程序
// 成功返回子进程id，否则返回 -1
// 错误：
//  1. 无效的文件名。
//  2. 进程池满/内存不足等资源错误。(暂不考虑)
pub fn sys_spawn(file: *const u8) -> isize {
    let token = current_user_token();
    let path = translated_str(token, file);
    if let Some(data) = get_app_data_by_name(path.as_str()) {
        let current_task = current_task().unwrap();
        let new_task = current_task.fork();
        let new_pid = new_task.pid.0;
        // modify trap context of new_task, because it returns immediately after switching
        let trap_cx = new_task.acquire_inner_lock().get_trap_cx();
        // we do not have to move to next instruction since we have done it before
        // for child process, fork returns 0
        trap_cx.x[10] = 0;
        // exec file
        new_task.exec(data);
        // add new task to scheduler
        add_task(new_task);
        new_pid as isize
    } else {
        -1
    }
}
