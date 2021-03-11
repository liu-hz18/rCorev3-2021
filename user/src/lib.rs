#![no_std]
#![feature(llvm_asm)]
#![feature(linkage)]
#![allow(dead_code)]
#![feature(panic_info_message)]
#![feature(alloc_error_handler)]

#[macro_use]
pub mod console;
mod syscall;
mod lang_items;

extern crate core;
#[macro_use]
extern crate bitflags;

use buddy_system_allocator::LockedHeap;
pub use console::{STDIN, STDOUT};
use syscall::*;

const USER_HEAP_SIZE: usize = 16384;

static mut HEAP_SPACE: [u8; USER_HEAP_SIZE] = [0; USER_HEAP_SIZE];

#[global_allocator]
static HEAP: LockedHeap = LockedHeap::empty();

#[alloc_error_handler]
pub fn handle_alloc_error(layout: core::alloc::Layout) -> ! {
    panic!("Heap allocation error, layout = {:?}", layout);
}

#[no_mangle]
#[link_section = ".text.entry"]
pub extern "C" fn _start() -> ! {
    unsafe {
        HEAP.lock()
            .init(HEAP_SPACE.as_ptr() as usize, USER_HEAP_SIZE);
    }
    exit(main()); // 使用它退出应用程序并将返回值告知 底层的批处理系统
    panic!("unreachable after sys_exit!");
}

#[linkage = "weak"]
#[no_mangle]
fn main() -> i32 {
    panic!("Cannot find main!");
}

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

impl TimeVal {
    pub fn new() -> Self {
        TimeVal { sec: 0, usec: 0 }
    }
}

pub fn write(fd: usize, buf: &[u8]) -> isize { sys_write(fd, buf) }
pub fn read(fd: usize, buf: &mut [u8]) -> isize { sys_read(fd, buf) }
pub fn exit(exit_code: i32) -> isize { sys_exit(exit_code) }
pub fn yield_() -> isize { sys_yield() }
pub fn get_time() -> isize {
    let time = TimeVal::new();
    match sys_get_time(&time, 0) {
        0 => ((time.sec & 0xffff) * 1000 + time.usec / 1000) as isize,
        _ => -1,
    }
}
pub fn set_priority(prio: isize) -> isize {
    sys_set_priority(prio)
}
pub fn sleep(period_ms: usize) {
    let start = get_time();
    while get_time() < start + period_ms as isize {
        sys_yield();
    }
}
pub fn mmap(start: usize, len: usize, prot: usize) -> isize {
    sys_mmap(start, len, prot)
}
pub fn munmap(start: usize, len: usize) -> isize {
    sys_munmap(start, len)
}
pub fn getpid() -> isize {
    sys_getpid()
}
pub fn fork() -> isize {
    sys_fork()
}
pub fn exec(path: &str) -> isize {
    sys_exec(path)
}
pub fn wait(exit_code: &mut i32) -> isize {
    sys_waitpid(-1, exit_code as *mut _)
}
pub fn waitpid(pid: usize, exit_code: &mut i32) -> isize {
    sys_waitpid(pid as isize, exit_code as *mut _)
}
pub fn spawn(path: &str) -> isize {
    sys_spawn(path)
}
