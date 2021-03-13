use crate::mm::{UserBuffer, translated_byte_buffer, translated_refmut, virtual_addr_range_printable, virtual_addr_range_writable};
use crate::task::{current_user_token, current_task_id, current_task, set_task_mail};
use crate::fs::{make_pipe};

// 由于内核和应用地址空间的隔离， sys_write 不再能够直接访问位于应用空间中的数据，而需要手动查页表才能知道那些 数据被放置在哪些物理页帧上并进行访问
// 安全检查：sys_write 仅能输出位于程序本身内存空间内的数据，否则报错
// write: 将缓冲区中的数据写入文件，最多将缓冲区中的数据全部写入，并返回直接写入的字节数
// 不仅仅局限于标准输入输出!!!
pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.acquire_inner_lock();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    // 在当前进程的文件描述符表中通过文件描述符找到某个文件
    // 无需关心文件具体的类型，只要知道它一定实现了 File Trait 的 read/write 方法即可
    if let Some(file) = &inner.fd_table[fd] {
        let file = file.clone();
        // release Task lock manually to avoid deadlock
        drop(inner);
        let (printable, start_pa, end_pa) = virtual_addr_range_printable(token, buf, len);
        if !printable {
            println!("[kernel] buffer overflow in APP {}, in sys_write! v_addr=[{:#x}, {:#x}), p_addr=[{:#x}, {:#x})", current_task_id(), buf as usize, buf as usize + len, start_pa, end_pa);
            return -1 as isize;
        }
        let buffers = translated_byte_buffer(token, buf, len);
        file.write(
            UserBuffer::new(buffers)
        ) as isize
    } else {
        println!("[kernel] Unsupported fd in sys_write!");
        -1
    }
}

// read: 从文件中读取数据放到缓冲区中，最多将缓冲区填满（即读取缓冲区的长度那么多字节），并返回实际读取的字节数
pub fn sys_read(fd: usize, buf: *const u8, len: usize) -> isize {
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.acquire_inner_lock();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        let file = file.clone();
        // release Task lock manually to avoid deadlock
        drop(inner);
        file.read(
            UserBuffer::new(translated_byte_buffer(token, buf, len))
        ) as isize
    } else {
        println!("[kernel] Unsupported fd in sys_read!");
        -1
    }
}

/// 功能：当前进程关闭一个文件。
/// 参数：fd 表示要关闭的文件的文件描述符。
/// 返回值：如果成功关闭则返回 0 ，否则返回 -1 。可能的出错原因：传入的文件描述符并不对应一个打开的文件。
/// syscall ID：57
/// 只有当一个管道的所有读端/写端都被关闭之后，管道占用的资源才会被回收，因此我们需要通过关闭文件的系统调用 sys_close 来尽可能早的关闭之后不再用到的读端和写端
pub fn sys_close(fd: usize) -> isize {
    let task = current_task().unwrap();
    let mut inner = task.acquire_inner_lock();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if inner.fd_table[fd].is_none() {
        return -1;
    }
    // 将进程控制块中的文件描述符表对应的一项改为 None 代表它已经空闲即可
    // 这也会导致内层的引用计数类型 Arc 被销毁，会减少一个文件的引用计数
    // 当引用计数减少到 0 之后文件所占用的资源就会被自动回收
    inner.fd_table[fd].take();
    0
}

// 父子进程间的单向进程间通信机制——管道
/// 功能：为当前进程打开一个管道。
/// 参数：pipe 表示应用地址空间中的一个长度为 2 的 usize 数组的起始地址，内核需要按顺序将管道读端
/// 和写端的文件描述符写入到数组中。
/// 返回值：如果出现了错误则返回 -1，否则返回 0 。可能的错误原因是：传入的地址不合法。
/// syscall ID：59
pub fn sys_pipe(pipe: *mut usize) -> isize {
    let task = current_task().unwrap();
    let token = current_user_token();
    let mut inner = task.acquire_inner_lock();
    let (pipe_read, pipe_write) = make_pipe();
    // 为读端和写端分配文件描述符并将它们放置在文件描述符表中的相应位置中
    let read_fd = inner.alloc_fd();
    inner.fd_table[read_fd] = Some(pipe_read);
    let write_fd = inner.alloc_fd();
    inner.fd_table[write_fd] = Some(pipe_write);
    drop(inner);
    // 读端和写端的文件描述符 写回到应用地址空间
    *translated_refmut(token, pipe) = read_fd;
    *translated_refmut(token, unsafe { pipe.add(1) }) = write_fd;
    0
}

// 基于邮箱的进程间通信
//  每个进程默认拥有唯一一个邮箱，基于“数据报文”收发字节信息，
//  利用环形buffer存储，读写顺序为 FIFO，不记录来源进程
//  每次读写单位必须为一个报文，如果缓冲区长度不够，舍弃超出的部分（也就是截断报文）
//  邮箱中最多拥有16条报文，每条报文最大长度256字节
//  当邮箱满时，发送邮件（也就是写邮箱）会失败
//  不考虑读写邮箱的权限，也就是所有进程都能够随意读写其他进程的邮箱。


// 读取本进程的一个报文，如果成功返回报文长度
// buf: 缓冲区头。len：缓冲区长度
// 邮箱自带读写功能，和进程绑定，不需要调用read/write来读写
// 邮箱依然作为一个文件描述符存在，资源是16个256Byte(u8)的报文段
pub fn sys_mail_read(buffer: *mut u8, len: usize) -> isize {
    // len > 256 按 256 处理，len < 队列首报文长度且不为0，则截断报文
    // len = 0，则不进行读取. 如果没有报文可读取，返回-1，否则返回0(len=0).
    // 邮箱空 或 buf无效: 返回-1
    // buf无效:
    let token = current_user_token();
    let (printable, _start_pa, _end_pa) = virtual_addr_range_printable(token, buffer, len);
    if !printable {
        return -1 as isize;
    }
    let task = current_task().unwrap();
    let mut inner = task.acquire_inner_lock();
    inner.mail_box.read(
        UserBuffer::new(translated_byte_buffer(token, buffer, len))
    ) as isize
}

// 向对应进程邮箱插入一条报文
// pid: 目标进程id, buf: 缓冲区头, len：缓冲区长度
pub fn sys_mail_write(pid: usize, buffer: *mut u8, len: usize) -> isize {
    // len > 256 按 256 处理
    // len = 0，则不进行写入，如果邮箱满，返回-1，否则返回0，这是用来测试是否可以发报
    // 可以向自己的邮箱写入报文
    // 邮箱满 或 buf无效: 返回-1
    let token = current_user_token();
    let writable = virtual_addr_range_writable(token, buffer, len);
    if !writable {
        return -1 as isize;
    }
    // 根据pid查找进程, 得到inner
    let buffer: UserBuffer = UserBuffer::new(translated_byte_buffer(token, buffer, len));
    if pid != current_task_id() {
        set_task_mail(pid, buffer)
    } else {
        let mut task = current_task().unwrap();
        let mut inner = task.acquire_inner_lock();
        inner.mail_box.write(buffer) as isize
    }
}
