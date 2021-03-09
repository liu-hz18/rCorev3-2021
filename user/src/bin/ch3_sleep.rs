#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;

use user_lib::{get_time, yield_};

#[no_mangle]
fn main() -> i32 {
    let current_timer = get_time();
    // 等待 3000ms 然后退出
    let wait_for = current_timer + 3000;
    // 通过 yield 来优化 轮询 (Busy Loop) 过程带来的 CPU 资源浪费
    while get_time() < wait_for {
        yield_();
    }
    println!("Test sleep OK!");
    0
}
