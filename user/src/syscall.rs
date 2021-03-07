const SYSCALL_WRITE: usize = 64;
const SYSCALL_EXIT: usize = 93;

fn syscall(id: usize, args: [usize; 3]) -> isize {
    let mut ret: isize; // 变量 ret 必须为可变 绑定，否则无法通过编译, 这也说明在 unsafe 块内编译器还是会进行力所能及的安全检查。
    // Trap 进入 S 模式执行批处理系统针对这个异常特别提供的服务代码
    // 这个接口可以被称为 ABI 或者 系统调用
    // 约定寄存器 a0~a6 (x10~x16) 保存系统调用的参数， a0~a1 (x10~x11) 保存系统调用的返回值
    // 寄存器 a7(x17) 用来传递 syscall ID，这是因为所有的 syscall 都是通过 ecall 指令触发的
    unsafe {
        llvm_asm!("ecall" // 触发 Environment call from U-mode 的异常
            : "={x10}" (ret) // output operands
            : "{x10}" (args[0]), "{x11}" (args[1]), "{x12}" (args[2]), "{x17}" (id) // input operands
            : "memory" // clobbers, 告诉编译器在执行嵌入汇编代码中的时候会修改内存, 防止编译器在不知情的情况下误优化
            : "volatile" // options, 嵌入汇编代码保持原样放到最终构建的可执行文件中
        );
    }
    ret
}

/// 功能：将内存中缓冲区中的数据写入文件。
/// 参数：`fd` 表示待写入文件的 文件描述符；
///      `buffer` 表示内存中缓冲区的 起始地址；胖指针, 里面既包含缓冲区的起始地址，还包含缓冲区的长度
///      `buffer.len()` 表示内存中缓冲区的 长度。
/// 返回值：返回成功写入的长度。
/// syscall ID：64
pub fn sys_write(fd: usize, buffer: &[u8]) -> isize {
    syscall(SYSCALL_WRITE, [fd, buffer.as_ptr() as usize, buffer.len()])
}


/// 功能：退出应用程序并将返回值告知批处理系统。
/// 参数：`exit_code` 表示应用程序的返回值。
/// 返回值：该系统调用不应该返回。
/// syscall ID：93
pub fn sys_exit(exit_code: i32) -> isize {
    syscall(SYSCALL_EXIT, [exit_code as usize, 0, 0])
}
