global_asm!(include_str!("switch.S"));

// 调用 __switch 之后直到它返回前的这段时间，
// 原 Trap 执行流会先被暂停并被 切换出去， CPU 转而运行另一个应用的 Trap 执行流。
// 之后在时机合适的时候，原 Trap 执行流才会从某一条 Trap 执行流 切换回来继续执行并最终返回

// __switch 和一个普通的函数之间的差别仅仅是它会换栈
// 封装成rust函数使得编译器可以帮你保存 调用者保存寄存器，我们只需要保存 被调用者保存寄存器 (类似一个函数的栈帧内容)
extern "C" {
    pub fn __switch(
        current_task_cx_ptr2: *const usize,
        next_task_cx_ptr2: *const usize
    );
}
