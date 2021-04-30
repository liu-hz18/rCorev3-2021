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
    current_task_id,
    current_user_token,
    current_trap_cx,
};
use crate::timer::set_next_trigger;
use crate::config::{TRAP_CONTEXT, TRAMPOLINE};

global_asm!(include_str!("trap.S"));

pub fn init() {
    set_kernel_trap_entry();
}

// 简单起见我们弱化了从 S 到 S 的 Trap ，省略了 Trap 上下文保存过程而直接 panic
fn set_kernel_trap_entry() {
    // 一旦进入内核后再次触发到 S 的 Trap，则会在硬件设置一些 CSR 之后跳过寄存器 的保存过程直接跳转到 trap_from_kernel 函数，在这里我们直接 panic 退出。
    unsafe {
        stvec::write(trap_from_kernel as usize, TrapMode::Direct);
    }
}

fn set_user_trap_entry() {
    // 把 stvec 设置为内核和应用地址空间共享的跳板页面的起始地址 TRAMPOLINE 而不是 编译器在链接时看到的 __alltraps 的地址
    // 因为启用分页模式之后我们只能通过跳板页面上的 虚拟地址 来实际取得 __alltraps 和 __restore 的汇编代码
    unsafe {
        stvec::write(TRAMPOLINE as usize, TrapMode::Direct);
    }
}

// 设置了 sie.stie 使得 S 特权级时钟中断不会被屏蔽
pub fn enable_timer_interrupt() {
    unsafe { sie::set_stimer(); }
}

pub fn disable_timer_interrupt() {
    unsafe { sie::clear_stimer(); }
}

#[no_mangle]
pub fn trap_handler() -> ! {
    // 将 stvec 修改为同模块下另一个函数 trap_from_kernel 的地址
    set_kernel_trap_entry(); 
    let scause = scause::read();
    let stval = stval::read();
    // 根据 scause 寄存器所保存的 Trap 的原因进行分发处理
    match scause.cause() {
        Trap::Exception(Exception::UserEnvCall) => {
            // jump to next instruction anyway
            let mut cx = current_trap_cx(); // 获取当前应用的 Trap 上下文的可变引用
            cx.sepc += 4; // 在 Trap 返回之后，我们希望应用程序执行流从 ecall 的下一条指令 开始执行
            // 这样在 __restore 的时候 sepc 在恢复之后就会指向 ecall 的下一条指令
            // get system call return value
            // 从 Trap 上下文取出作为 syscall ID 的 a7 和系统调用的三个参数 a0~a2 传给 syscall 函数并获取返回值
            let result = syscall(cx.x[17], [cx.x[10], cx.x[11], cx.x[12], cx.x[13], cx.x[14]]) as usize;
            // cx is changed during sys_exec, so we have to call it again
            // 对于系统调用 sys_exec 来说，一旦调用它之后，我们会发现 trap_handler 原来上下文中的 cx 失效了
            // 因为它是用来访问 之前地址空间 中 Trap 上下文被保存在的那个物理页帧的, 而现在它已经被回收掉了
            // 所以我们 需要重新获取 cx
            cx = current_trap_cx();
            cx.x[10] = result as usize;
        }
        Trap::Exception(Exception::StoreFault) |
        Trap::Exception(Exception::StorePageFault) => {
            info!(
                "[kernel] {:?} in application, bad addr = {:#x}, bad instruction = {:#x}, core dumped.",
                scause.cause(),
                stval,
                current_trap_cx().sepc,
            );
            info!("[kernel] Store PageFault in Application {} (killed), core dumped.", current_task_id());
            exit_current_and_run_next(-2); // 直接切换并运行下一个 应用程序
        },
        Trap::Exception(Exception::LoadFault) |
        Trap::Exception(Exception::LoadPageFault) => {
            info!(
                "[kernel] {:?} in application, bad addr = {:#x}, bad instruction = {:#x}, core dumped.",
                scause.cause(),
                stval,
                current_trap_cx().sepc,
            );
            info!("[kernel] Load PageFault in Application {} (killed), core dumped.", current_task_id());
            exit_current_and_run_next(-2); // 直接切换并运行下一个 应用程序
        },
        Trap::Exception(Exception::InstructionFault) |
        Trap::Exception(Exception::InstructionPageFault) => {
            info!(
                "[kernel] {:?} in application, bad addr = {:#x}, bad instruction = {:#x}, core dumped.",
                scause.cause(),
                stval,
                current_trap_cx().sepc,
            );
            info!("[kernel] Instruction PageFault in Application {} (killed), core dumped.", current_task_id());
            exit_current_and_run_next(-2); // 直接切换并运行下一个 应用程序
        },
        Trap::Exception(Exception::IllegalInstruction) => {
            info!("[kernel] IllegalInstruction in Application {} (killed), core dumped.", current_task_id());
            exit_current_and_run_next(-3);
        },
        // 抢占式调度
        // 中断不会被屏蔽，而是 Trap 到 S 特权级内的我们的 trap_handler 里面进行处理，并顺利切换到下一个应用
        Trap::Interrupt(Interrupt::SupervisorTimer) => {
            set_next_trigger(); // 重新设置一个 10ms 的计时器
            suspend_current_and_run_next(); // 暂停当前应用并切换到下一个
        },
        _ => {
            panic!("Unsupported trap {:?}, stval = {:#x}!, Application {} (killed)", scause.cause(), stval, current_task_id());
        }
    }
    trap_return();
}

//  完成 Trap 处理之后，我们需要调用 trap_return 返回用户态
#[no_mangle]
pub fn trap_return() -> ! {
    set_user_trap_entry(); // 让应用 Trap 到 S 的时候可以跳转到 __alltraps
    let trap_cx_ptr = TRAP_CONTEXT; // Trap 上下文在应用地址空间中的虚拟地址
    let user_satp = current_user_token(); // 要继续执行的应用 地址空间的 token 
    extern "C" {
        fn __alltraps();
        fn __restore();
    }
    // __restore 在内核/应用地址空间中共同的虚拟地址
    // __alltraps 是对齐到地址空间跳板页面的起始地址 TRAMPOLINE 上的， 
    // 则 __restore 的虚拟地址只需在 TRAMPOLINE 基础上加上 __restore 相对于 __alltraps 的偏移量即可。
    let restore_va = __restore as usize - __alltraps as usize + TRAMPOLINE;
    unsafe {
        llvm_asm!("fence.i" :::: "volatile");
        llvm_asm!("jr $0" 
            :: "r"(restore_va), "{a0}"(trap_cx_ptr), "{a1}"(user_satp) 
            :: "volatile"
        );
    }
    panic!("Unreachable in back_to_user!");
}

#[no_mangle]
pub fn trap_from_kernel() -> ! {
    panic!("a trap {:?} from kernel!", scause::read().cause());
}

pub use context::TrapContext;
