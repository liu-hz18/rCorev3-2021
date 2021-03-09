mod context;

use riscv::register::{
    mtvec::TrapMode,
    stvec,
    scause::{
        self,
        Trap,
        Exception,
        Interrupt,
    },
    stval,
    sie,
};
use crate::syscall::syscall;
use crate::task::{
    exit_current_and_run_next,
    suspend_current_and_run_next,
};
use crate::timer::set_next_trigger;

global_asm!(include_str!("trap.S"));

// 设置了 sie.stie 使得 S 特权级时钟中断不会被屏蔽
pub fn enable_timer_interrupt() {
    unsafe { sie::set_stimer(); }
}

pub fn init() {
    extern "C" { fn __alltraps(); }
    unsafe {
        // 将 stvec 设置为 Direct 模式指向它的地址
        stvec::write(__alltraps as usize, TrapMode::Direct);
    }
}

#[no_mangle]
pub fn trap_handler(cx: &mut TrapContext) -> &mut TrapContext {
    let scause = scause::read();
    let stval = stval::read();
    // 根据 scause 寄存器所保存的 Trap 的原因进行分发处理
    match scause.cause() {
        Trap::Exception(Exception::UserEnvCall) => {
            cx.sepc += 4; // 在 Trap 返回之后，我们希望应用程序执行流从 ecall 的下一条指令 开始执行
            // 这样在 __restore 的时候 sepc 在恢复之后就会指向 ecall 的下一条指令

            // 从 Trap 上下文取出作为 syscall ID 的 a7 和系统调用的三个参数 a0~a2 传给 syscall 函数并获取返回值
            cx.x[10] = syscall(cx.x[17], [cx.x[10], cx.x[11], cx.x[12]]) as usize;
        }
        Trap::Exception(Exception::StoreFault) |
        Trap::Exception(Exception::StorePageFault) => {
            println!("[kernel] PageFault in application, core dumped.");
            // 直接切换并运行下一个 应用程序
            exit_current_and_run_next();
        }
        Trap::Exception(Exception::IllegalInstruction) => {
            println!("[kernel] IllegalInstruction in application, core dumped.");
            exit_current_and_run_next();
        },
        // 抢占式调度
        // 中断不会被屏蔽，而是 Trap 到 S 特权级内的我们的 trap_handler 里面进行处理，并顺利切换到下一个应用
        Trap::Interrupt(Interrupt::SupervisorTimer) => {
            set_next_trigger(); // 重新设置一个 10ms 的计时器
            suspend_current_and_run_next(); // 暂停当前应用并切换到下一个
        },
        _ => {
            panic!("Unsupported trap {:?}, stval = {:#x}!", scause.cause(), stval);
        }
    }
    cx // 将传入的 cx 原样返回
}

pub use context::TrapContext;