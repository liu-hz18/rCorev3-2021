# Trap 上下文保存/恢复的汇编代码
# 这段汇编代码放在一个物理页帧中
# __alltraps 恰好位于这个物理页帧的开头，其物理地址被外部符号 strampoline 标记
# 这段汇编代码 被放在它们地址空间的最高页面上
# 由于这段汇编代码在执行的时候涉及到地址空间切换，故而被称为跳板页面
# 注意无论是内核还是应用的地址空间，跳板页面均位于同样位置，且它们也将会映射到同一个实际存放这段 汇编代码的物理页帧
# 也就是说，无论在执行 __alltraps 还是 __restore 切换地址空间的时候，两个地址空间 在切换地址空间的指令附近的映射方式均是相同的，这就说明了指令仍是连续执行的

.altmacro
.macro SAVE_GP n
    sd x\n, \n*8(sp)
.endm
.macro LOAD_GP n
    ld x\n, \n*8(sp)
.endm
    .section .text.trampoline # 在调整内存布局的时候将它对齐到代码段的一个页面中
    .globl __alltraps
    .globl __restore
    .align 2

# 将 Trap 上下文保存在内核栈上
__alltraps:
    # 当 __alltraps 保存 Trap 上下文的时候，我们必须通过修改 satp 从应用地址空间切换到内核地址空间， 因为 trap handler 只有在内核地址空间中才能访问
    # 在这一行之前 sp 指向用户栈， sscratch 指向sscratch 则被设置为 指向应用地址空间中存放 Trap 上下文的位置，实际在次高页面
    
    # 交换 sscratch 和 sp 
    csrrw sp, sscratch, sp
    # 现在 sp 指向 *TrapContext in user space, sscratch 指向用户栈
    
    # allocate a TrapContext on kernel stack
    # addi sp, sp, -34*8
    # save general-purpose registers
    sd x1, 1*8(sp)
    # skip sp(x2), we will save it later
    sd x3, 3*8(sp)
    # skip tp(x4), application does not use it
    # save x5~x31
    .set n, 5
    .rept 27
        SAVE_GP %n
        .set n, n+1
    .endr

    # we can use t0/t1/t2 freely, because they were saved on kernel stack
    # 将 CSR sstatus 和 sepc 的值分别读到寄存器 t0 和 t1 中然后保存到内核栈对应的位置上
    csrr t0, sstatus
    csrr t1, sepc
    sd t0, 32*8(sp)
    sd t1, 33*8(sp)
    # read user stack from sscratch and save it on the kernel stack
    # 首先将 sscratch 的值读到寄存器 t2 并保存到内核栈上，注意它里面是进入 Trap 之前的 sp 的值，指向 用户栈
    csrr t2, sscratch
    sd t2, 2*8(sp)
    # 至此，在内核地址空间中完成了 向应用地址空间的次高页面 保存 Trap 上下文的工作

    # load kernel_satp into t0，将内核地址空间的 token 载入到 t0 寄存器中
    ld t0, 34*8(sp)
    # load trap_handler into t1， 将 trap handler 入口点的虚拟地址载入到 t1 寄存器中
    ld t1, 36*8(sp)
    # move to kernel_sp，将 sp 修改为应用内核栈顶的地址
    ld sp, 35*8(sp)
    # switch to kernel space， 将 satp 修改为内核地址空间的 token 并使用 sfence.vma 刷新快表
    csrw satp, t0
    sfence.vma
    # 至此，切换到了内核地址空间
    # jump to trap_handler (t1)
    jr t1 # 跳转到使用 Rust 编写的 trap_handler 函数 完成 Trap 分发及处理
    # 跳转指令实际被执行时的虚拟地址 和 在编译器进行链接时看到的它的地址不同

# 从保存在内核栈上的 Trap 上下文恢复寄存器
# 在 __restore 恢复 Trap 上下文的时候，我们也必须从内核地址空间切换回应用地址空间，因为应用的代码和 数据只能在它自己的地址空间中才能访问
# 这就要求应用和内核地址空间在切换地址空间指令附近是平滑的
# 两个参数：
#   a0: Trap 上下文在应用 地址空间中的位置， 这个对于所有的应用来说都是相同的
#   a1: 即将回到的应用的地址空间 的 token 
__restore:
    # case1: start running app by __restore
    # case2: back to U after handling trap
    
    # a0 在调用 trap_handler 前后并没有发生变化，仍然指向分配 Trap 上下文之后的内核栈栈顶，和此时 sp 的值相同
    # 在 __switch 之后， sp 就已经正确指向了我们需要的 Trap 上下文地址, 不需要下面这条指令了
    # mv sp, a0 

    # 此时 sp 仍然指向内核栈的栈顶
    # now sp->kernel stack(after allocated), sscratch->user stack
    
    # a0: *TrapContext in user space(Constant); a1: user space token
    # switch to user space
    # 先切换回应用地址空间
    csrw satp, a1
    sfence.vma

    # 将传入的 Trap 上下文位置保存在 sscratch 寄存器中，这样 __alltraps 中才能基于它将 Trap 上下文 保存到正确的位置
    csrw sscratch, a0
    # 将 sp 修改为 Trap 上下文的位置，后面基于它恢复各通用寄存器和 CSR
    mv sp, a0
    # now sp points to TrapContext in user space, start restoring based on it

    # 从内核栈顶的 Trap 上下文恢复通用寄存器和 CSR
    # restore sstatus/sepc
    # 先恢复 CSR 再恢复通用寄存器，这样我们使用的三个临时寄存器 才能被正确恢复
    ld t0, 32*8(sp)
    ld t1, 33*8(sp)
    csrw sstatus, t0
    csrw sepc, t1
    # restore general-purpuse registers except sp/tp
    ld x1, 1*8(sp)
    ld x3, 3*8(sp)
    .set n, 5
    .rept 27
        LOAD_GP %n
        .set n, n+1
    .endr
    # back to user stack
    ld sp, 2*8(sp)
    sret  # 通过一条 sret 指令回到U-mode 应用程序执行
