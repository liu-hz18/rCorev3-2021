
#![no_std] // Rust 编译器不使用 Rust 标准库 std 转而使用核心库 core
#![no_main] // 告诉编译器我们没有一般意义上的 main 函数
#![feature(global_asm)] // 嵌入全局汇编代码
#![feature(llvm_asm)]
#![feature(panic_info_message)]
#![feature(const_in_array_repeat_expressions)]
#![feature(alloc_error_handler)]

// use log::{error, warn, info, debug, trace};

#[macro_use]
extern crate bitflags;
extern crate log;
extern crate alloc;

#[macro_use]
mod console;
mod lang_items; // 引入模块
mod sbi;
mod logging;
mod syscall;
mod trap;
mod task;
mod config;
mod timer;
mod mm;
mod fs;
mod drivers;

// 将同目录下的汇编代码 entry.asm 转化为字符串并通过 global_asm! 宏嵌入到代码中
global_asm!(include_str!("entry.asm"));
global_asm!(include_str!("link_app.S"));

fn clear_bss() {
    extern "C" {
        fn sbss();
        fn ebss();
    }
    (sbss as usize..ebss as usize).for_each(|a| 
        unsafe { (a as *mut u8).write_volatile(0) }
    );
}

// 对应用程序而言，需要限制的主要有两个方面：
// 1. 应用程序不能访问任意的地址空间
// 2. 应用程序不能执行某些可能破会计算机系统的指令

#[no_mangle] // 避免编译器对 rust_main 的名字进行混淆, 不然会链接失败
pub fn rust_main() -> ! {
    // extern "C" {
    //     fn stext();
    //     fn etext();
    //     fn srodata();
    //     fn erodata();
    //     fn sdata();
    //     fn edata();
    //     fn sbss();
    //     fn ebss();
    //     fn boot_stack();
    //     fn boot_stack_top();
    // }
    // 在执行环境调用 应用程序的 rust_main 主函数前，把 .bss 段的全局数据清零
    // 在程序内自己进行清零的时候，我们就不用去解析 ELF 了。而是通过链接脚本 linker.ld 中给出的全局符号 sbss 和 ebss 来确定 .bss 段的位置
    clear_bss();
    // 输出 os 内存空间布局
    // info!(".text [{:#x}, {:#x})", stext as usize, etext as usize);
    // info!(".rodata [{:#x}, {:#x})", srodata as usize, erodata as usize);
    // info!(".data [{:#x}, {:#x})", sdata as usize, edata as usize);
    // info!(
    //     "boot_stack [{:#x}, {:#x})",
    //     boot_stack as usize, boot_stack_top as usize
    // );
    // info!(".bss [{:#x}, {:#x})", sbss as usize, ebss as usize);
    // dbgx!(boot_stack_top as usize - boot_stack as usize);

    println!("[kernel] Hello, world!");
    mm::init();
    mm::remap_test();
    println!("[kernel] after initproc!");

    // logging::init();
    trap::init();

    trap::enable_timer_interrupt(); // 设置了 sie.stie 使得 S 特权级时钟中断不会被屏蔽
    timer::set_next_trigger(); // 设置第一个 10ms 的计时器
    fs::list_apps();
    
    task::add_initproc();
    task::run_tasks();
    panic!("Unreachable in rust_main!");
}
