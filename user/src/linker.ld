
OUTPUT_ARCH(riscv)
ENTRY(_start)

/* 一个地址空间中的虚拟地址而非物理地址 */
BASE_ADDRESS = 0x0;

SECTIONS
{
    . = BASE_ADDRESS;
    /* 从 0x0 开始向高地址放置应用内存布局中的 各个逻辑段 */
    .text : {
        *(.text.entry)
        *(.text .text.*)
    }
    . = ALIGN(4K); /* 页面对齐, 因为前后两个逻辑段的访问方式限制是不同的，由于我们只能以页为单位对这个限制进行设置 */
    .rodata : {
        *(.rodata .rodata.*)
    }
    . = ALIGN(4K);
    /* .data 和 .bss 两个逻辑段由于限制相同，它们中间 则无需进行页面对齐  */
    .data : {
        *(.data .data.*)
    }
    .bss : {
        *(.bss .bss.*)
    }
    /DISCARD/ : {
        *(.eh_frame)
        *(.debug*)
    }
}
