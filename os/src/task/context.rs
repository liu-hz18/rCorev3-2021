#[repr(C)]
pub struct TaskContext {
    ra: usize,
    s: [usize; 12],
}

// 只保存了 ra 和被调用者保存的 s0~s11
// ra 记录了 __switch 返回之后应该到哪里继续执行， 从而在切换回来并 ret 之后能到正确的位置，这里就是 __restore
// 保存 被调用者保存寄存器 s0-11: 因为调用者保存的寄存器可以由编译器帮我们自动保存
impl TaskContext {
    // 初始化任务上下文
    pub fn goto_restore() -> Self {
        extern "C" { fn __restore(); }
        // 将任务上下文的 ra 寄存器设置为 __restore 的入口地址, 在 __switch 从它上面恢复并返回 之后就会直接跳转到 __restore
        // 在 __switch 之后， sp 就已经正确指向了我们需要的 Trap 上下文地址
        Self {
            ra: __restore as usize,
            s: [0; 12],
        }
    }
}

