# OS-lab1 report

刘泓尊  2018011446  计84

## Chapter1实现的内容

1. 完成了实验环境的配置和工具链安装
2. 移除了标准库依赖，按照tutorial-book实现了OS的起始逻辑，迁移到了裸机运行环境
3. 实现了console.rs中的print的功能，以及对应的sbi_call系统调用接口。
4. 实现了彩色化LOG的输出功能，支持error, warn, info, debug, trace。代码位于logging.rs
5. 在makefile中加入了方便调试的UNOPTIMIZED+DEBUG模式编译命令`make run-debug`， 加入了方便使用二进制调试工具`objdump, readobj, file, addr2line`等工具的脚本

## 彩色化 LOG

借助Cargo crate`log = "0.4"`这个库实现了彩色化LOG的功能。

为了实现彩色化输出，我实现了print_in_color函数和with_color宏，输出带有颜色的文本。其次实现了SimpleLogger类来实现Log接口，在调用error!等宏时进行等级判断等逻辑。实现了logging::init()函数来在OS初始化的时候获取LOG环境参数，设置运行时的LOG等级，以实现等级过滤的目的。

**默认LOG等级是INFO**。支持的LOG参数有`[ERROR, WARN, INFO, DEBUG, TRACE, OFF]`。OFF表示关闭所有LOG。(参数均大写)

我在main.rs中添加了如下代码用于测试：

<img src="C:\Users\lenovo\AppData\Roaming\Typora\typora-user-images\image-20210307161822045.png" alt="image-20210307161822045" style="zoom:67%;" />

<img src="C:\Users\lenovo\AppData\Roaming\Typora\typora-user-images\image-20210307161709444.png" alt="image-20210307161709444" style="zoom:67%;" />

之后在命令行测试效果:

1. 执行命令`make debug LOG=ERROR`:

    <img src="C:\Users\lenovo\AppData\Roaming\Typora\typora-user-images\image-20210307162607133.png" alt="image-20210307162607133" style="zoom:67%;" />

2. 执行命令`make debug LOG=WARN`:

    <img src="C:\Users\lenovo\AppData\Roaming\Typora\typora-user-images\image-20210307162631683.png" alt="image-20210307162631683" style="zoom:67%;" />

3. 执行命令`make debug LOG=INFO`:

    <img src="C:\Users\lenovo\AppData\Roaming\Typora\typora-user-images\image-20210307162656166.png" alt="image-20210307162656166" style="zoom:67%;" />![image-20210307162719781](C:\Users\lenovo\AppData\Roaming\Typora\typora-user-images\image-20210307162719781.png)

4. 执行命令`make debug LOG=DEBUG`:

    <img src="C:\Users\lenovo\AppData\Roaming\Typora\typora-user-images\image-20210307162815457.png" alt="image-20210307162815457" style="zoom:67%;" />

5. 执行命令`make debug LOG=TRACE`：

    <img src="C:\Users\lenovo\AppData\Roaming\Typora\typora-user-images\image-20210307162741825.png" alt="image-20210307162741825" style="zoom:67%;" />

## 问答作业

1. ##### 为了方便 os 处理，Ｍ态软件会将 S 态异常/中断委托给 S 态软件，请指出有哪些寄存器记录了委托信息，rustsbi 委托了哪些异常/中断？（也可以直接给出寄存器的值）

    riscv使用`mideleg`, `medeleg`分别将中断和异常从M态委托到S态.

    `mideleg` 指示中断委托，每一位是否为1表示对应编号的中断是否委托给S态。`medeleg` 则对应异常委托。

    `mideleg` 和 `medeleg` 的值在 RustSBI 启动时以控制台输出信息的方式给出：

    <img src="D:\大三下\操作系统\作业\异常中断委托.PNG" alt="异常中断委托" style="zoom:67%;" />

    **中断委托向量** `mideleg` = `0x0222` = `0b 0000_0010_0010_0010`

    **异常委托向量** `medeleg ` = `0xb1ab` = `0b 1011_0001_1010_1011`

    从riscv 特权态手册中的Exception Code (section 3.1.16 page 37)可以看到:

    **中断**委托了

    - Supervisor software interrupt (code=1)
    - Supervisor timer interrupt (code=5)
    - Supervisor external interrupt (code=9)

    **异常**委托了

    - Instruction address misaligned (code = 0)
    - Instruction access fault (code = 1)
    - Breakpoint (code = 3)
    - Load access fault (code = 5)
    - Store/AMO access fault (code = 7)
    - Environment call from U-mode (code = 8)
    - Instruction page fault (code = 12)
    - Load page fault (code = 13)
    - Store/AMO page fault (code = 15)

2. ##### 请学习 gdb 调试工具的使用，并通过 gdb 简单跟踪从机器加电到跳转到 `0x80200000` 的简单过程。只需要描述重要的跳转即可，只需要描述在 qemu 上的情况。

   a. **加电后跳转到`0x8000_0000`**: 
   
   运行 `make debug` 开启GDB调试之后，我们首先来看最开始的10条指令。`0x1000`处将`t0`设置为`0x1000`, 之后在`0x100c`处将`t0`加载为`0x101a`处的地址`0x8000_0000` , 之后在 `0x1010` 处跳转到了地址t0 =`0x8000_0000 `. 这个地址就是RustSBI的起始地址。
   
   <img src="C:\Users\lenovo\AppData\Roaming\Typora\typora-user-images\image-20210307151435381.png" alt="image-20210307151435381" style="zoom: 67%;" />
   
   b. **`0x8000_0000`到RustSBI::main()**: 
   
   对比`0x8000_0000`处的汇编代码可以发现，它正是RustSBI 的 `start()`函数。
   
   <img src="C:\Users\lenovo\AppData\Roaming\Typora\typora-user-images\image-20210307152301598.png" alt="image-20210307152301598" style="zoom: 67%;" />
   
   ​					  <img src="C:\Users\lenovo\AppData\Roaming\Typora\typora-user-images\image-20210307152339541.png" alt="image-20210307152339541" style="zoom: 67%;" />

​		在`start()`函数最后，跳转到了 **RustSBI::main()** 函数(in rustsbi/platform/qemu/src/main.rs)：

<img src="C:\Users\lenovo\AppData\Roaming\Typora\typora-user-images\image-20210307152546549.png" alt="image-20210307152546549" style="zoom: 67%;" />

​		使用GDB反汇编可以得到 **main** 的入口地址是`0x8000_2572`.

<img src="C:\Users\lenovo\AppData\Roaming\Typora\typora-user-images\image-20210307152724154.png" alt="image-20210307152724154" style="zoom:67%;" />

​		

​       c. **`RustSBI::main()`到`s_mode_start()`:**

​		进入main()函数之后，在main()函数最后，设置了`mepc `= `s_mode_start`, 这也是mret命令将会跳转到的地址。最后调用了`enter_privileged()` (in rustsbi/rustsbi/src/privileged.rs）函数。

​										<img src="C:\Users\lenovo\AppData\Roaming\Typora\typora-user-images\image-20210307153049423.png" alt="image-20210307153049423" style="zoom:67%;" />

​		我们再来看`enter_privileged`函数, 该函数最后调用了`mret`，也就是跳转到了之前设置的`mepc`向量，即`s_mode_start` (in rustsbi/platform/qemu/src/main.rs). 

​								<img src="C:\Users\lenovo\AppData\Roaming\Typora\typora-user-images\image-20210307153405038.png" alt="image-20210307153405038" style="zoom:67%;" />

​		使用GDB获得**`s_mode_start`**(也就是此时`mepc`)的地址是`0x800023da`：

​										<img src="C:\Users\lenovo\AppData\Roaming\Typora\typora-user-images\image-20210307154701231.png" alt="image-20210307154701231" style="zoom:67%;" />

​		d. **`s_mode_start`到`0x8020_0000`:**

​		`s_mode_start`通过`jr ra`进行了一次跳转，对应的源代码和反汇编如下：

​													<img src="C:\Users\lenovo\AppData\Roaming\Typora\typora-user-images\image-20210307153627219.png" alt="image-20210307153627219" style="zoom:67%;" />

<img src="C:\Users\lenovo\AppData\Roaming\Typora\typora-user-images\image-20210307155020657.png" alt="image-20210307155020657" style="zoom:67%;" />

​        在`jr ra`之前设置了`ra`的值，可以看到最终`ra`等于`0x8000_23ea`处的数值`0x8020_0000`, 最终`jr ra`就跳到了**`0x8020_0000`**, 也就是我们自己实现的**OS起始代码**。

​		e. **`0x8020_0000`到OS 的 `rust_main()`**

​		之后就进入了.stext段，而里面第一个被放置的又是来自 `entry.asm` 中的段 .text.entry，也就是OS入口点`_start`，之后就跳转到了`rust_main()`, 开始执行OS的rust部分的代码。

​						<img src="C:\Users\lenovo\AppData\Roaming\Typora\typora-user-images\image-20210307155656317.png" alt="image-20210307155656317" style="zoom:67%;" />