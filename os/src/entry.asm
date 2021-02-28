# 添加建立 栈 的代码逻辑 
    .section .text.entry // addr = 0x80200000
    .globl _start
_start: // _start 是整个程序的入口点
    la sp, boot_stack_top // 将 sp 设置为我们预留的栈空间的栈顶位置
    call rust_main // 调用 rust_main
    // 以上这两条指令单独作为一个名为 .text.entry 的段

// 栈 从 高地址 到 低地址 增长
    .section .bss.stack
    .globl boot_stack
boot_stack: // 低地址：可用栈 下边界 被全局符号 boot_stack 标识
    .space 4096 * 16 // 预留了一块大小为 4096 * 16 字节也就是 64KiB 的空间用作接下来要运行的程序的栈空间
    .globl boot_stack_top
boot_stack_top: // 高地址：栈空间的栈顶地址被全局符号 boot_stack_top 标识，向下增长，直到 boot_stack
