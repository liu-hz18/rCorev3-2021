use crate::sbi::shutdown;
use core::panic::PanicInfo;

#[panic_handler] //提供 panic 处理函数的实现并通过标记通知编译器采用我们的实现
fn panic(info: &PanicInfo) -> ! {
    // 给异常处理函数 panic 增加显示字符串能力
    if let Some(location) = info.location() {
        println!( // 显示报错位置
            "[kernel] Panicked at \x1b[31m{}:{}\x1b[0m \x1b[93m{}\x1b[0m",
            location.file(),
            location.line(),
            info.message().unwrap()
        );
    } else {
        println!("[kernel] Panicked: \x1b[93m{}\x1b[0m", info.message().unwrap());
    }
    shutdown()
}
