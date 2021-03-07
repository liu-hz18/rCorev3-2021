use riscv::register::sstatus::{Sstatus, self, SPP};

#[repr(C)]
pub struct TrapContext {
    // 然在 Trap 控制流中只是会执行 Trap 处理 相关的代码，但依然可能直接或间接调用很多模块，因此很难甚至不可能找出哪些寄存器无需保存。
    pub x: [usize; 32], // 全部保存
    // scause/stval 的情况是：它总是在 Trap 处理的第一时间就被使用或者是在其他地方保存下来了，因此它没有被修改并造成不良影响的风险。
    // 对于 sstatus/sepc 而言，它们会在 Trap 处理的全程有意义, 在 Trap 执行流最后 sret 的时候还用到了它们
    // 而且确实会出现 Trap 嵌套的情况使得它们的值被覆盖掉
    pub sstatus: Sstatus,
    pub sepc: usize,
}
 
impl TrapContext {
    pub fn set_sp(&mut self, sp: usize) { self.x[2] = sp; }

    pub fn app_init_context(entry: usize, sp: usize) -> Self {
        let mut sstatus = sstatus::read();
        sstatus.set_spp(SPP::User); // 将 sstatus 寄存器的 SPP 字段设置为 User 
        let mut cx = Self {
            x: [0; 32],
            sstatus,
            sepc: entry, // 修改其中的 sepc 寄存器为应用程序入口点 entry
        };
        cx.set_sp(sp);
        cx
    }
}
