# Trap 上下文保存/恢复的汇编代码
.altmacro
.macro SAVE_GP n
    sd x\n, \n*8(sp)
.endm
.macro LOAD_GP n
    ld x\n, \n*8(sp)
.endm
    .section .text
    .globl __alltraps
    .globl __restore
    .align 2

# 将 Trap 上下文保存在内核栈上
__alltraps:
    # 在这一行之前 sp 指向用户栈， sscratch 指向内核栈
    # 交换 sscratch 和 sp 
    csrrw sp, sscratch, sp
    # 现在 sp 指向内核栈， sscratch 指向用户栈
    # now sp->kernel stack, sscratch->user stack
    
    # allocate a TrapContext on kernel stack
    addi sp, sp, -34*8
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

    # set input argument of trap_handler(cx: &mut TrapContext)
    mv a0, sp
    # 让寄存器 a0 指向内核栈的栈指针也就是我们刚刚保存的 Trap 上下文的地址， 这是由于我们接下来要调用 trap_handler 进行 Trap 处理，它的第一个参数 cx 由调用规范要从 a0 中获取
    call trap_handler # 跳转到使用 Rust 编写的 trap_handler 函数 完成 Trap 分发及处理

# 从保存在内核栈上的 Trap 上下文恢复寄存器
__restore:
    # case1: start running app by __restore
    # case2: back to U after handling trap
    
    # a0 在调用 trap_handler 前后并没有发生变化，仍然指向分配 Trap 上下文之后的内核栈栈顶，和此时 sp 的值相同
    # 在 __switch 之后， sp 就已经正确指向了我们需要的 Trap 上下文地址, 不需要下面这条指令了
    # mv sp, a0 

    # 此时 sp 仍然指向内核栈的栈顶
    # now sp->kernel stack(after allocated), sscratch->user stack
    
    # 从内核栈顶的 Trap 上下文恢复通用寄存器和 CSR
    # restore sstatus/sepc
    # 先恢复 CSR 再恢复通用寄存器，这样我们使用的三个临时寄存器 才能被正确恢复
    ld t0, 32*8(sp)
    ld t1, 33*8(sp)
    ld t2, 2*8(sp)
    csrw sstatus, t0
    csrw sepc, t1
    csrw sscratch, t2
    # restore general-purpuse registers except sp/tp
    ld x1, 1*8(sp)
    ld x3, 3*8(sp)
    .set n, 5
    .rept 27
        LOAD_GP %n
        .set n, n+1
    .endr
    # 此时 sp 指向保存了 Trap 上下文之后的内核栈栈顶， sscratch 指向用户栈栈顶
    # release TrapContext on kernel stack
    addi sp, sp, 34*8
    # now sp->kernel stack, sscratch->user stack
    # 交换 sscratch 和 sp，现在 sp 重新指向用户栈栈顶，sscratch 也依然保存 进入 Trap 之前的状态并指向内核栈栈顶
    csrrw sp, sscratch, sp
    sret # 通过一条 sret 指令回到U-mode 应用程序执行
