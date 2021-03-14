#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;

use user_lib::{
    fork,
    wait,
    exec,
    yield_,
};

#[no_mangle]
fn main() -> i32 {
    if fork() == 0 {
        // 我们需要在字符串末尾手动加入 \0 ，因为 Rust 在将这些字符串连接到只读数据段的时候不会插入 \0
        exec("ch5_usershell\0", &[0 as *const u8]);
    } else { // 返回值不为 0 的分支，表示调用 fork 的初始进程自身
        loop {
            let mut exit_code: i32 = 0;
            // 它在不断循环调用 wait 来等待那些被移交到它下面的子进程并回收它们占据的资源
            let pid = wait(&mut exit_code);
            if pid == -1 {
                yield_(); // 初始进程对于资源的回收并不算及时，但是对于已经退出的僵尸进程，初始进程最终总能够成功回收它们的资源
                continue;
            }
            println!(
                "[initproc] Released a zombie process, pid={}, exit_code={}",
                pid,
                exit_code,
            );
        }
    }
    0
}