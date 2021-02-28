/* 目标平台为 riscv */
OUTPUT_ARCH(riscv)
/* 整个程序的入口点为之前定义的全局符号 _start */
ENTRY(_start)
/* 初始化代码被放在的地址 */
BASE_ADDRESS = 0x80200000;

/* 从 低地址 到 高地址 排列 */
SECTIONS
{
    . = BASE_ADDRESS;
    skernel = .;
    /* 标定 开始和结束地址 */
    stext = .;
    /* 第一个被放置的 是 .text ，而里面第一个被放置的又是来自 entry.asm 中的段 .text.entry 
        这个段恰恰是含有两条指令的执行环境初始化代码， 它在所有段中最早被放置在我们期望的 0x80020000 处
    */
    .text : {
        *(.text.entry)
        *(.text .text.*)
    }

    . = ALIGN(4K);
    etext = .;

    srodata = .;
    .rodata : {
        *(.rodata .rodata.*)
    }

    . = ALIGN(4K);
    erodata = .;

    sdata = .;
    .data : {
        *(.data .data.*)
    }

    . = ALIGN(4K);
    edata = .;

    .bss : {
        /* entry.asm 中分配的栈空间对应的段 .bss.stack 被放入到可执行文件中的 .bss 段中的低地址中 */
        *(.bss.stack)
        sbss = .;
        *(.bss .bss.*)
    }

    . = ALIGN(4K);
    ebss = .; /* 注意: 地址区间 [sbss,ebss) 并不包括栈空间 */

    ekernel = .;

    /DISCARD/ : {
        *(.eh_frame)
    }
}
