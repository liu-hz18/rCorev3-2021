use crate::mm::{
    UserBuffer,
    translated_byte_buffer,
    translated_refmut,
    virtual_addr_range_printable,
    virtual_addr_range_writable,
    virtual_addr_writable,
    translated_str,
    translated_virtual_ptr
};
use crate::task::{current_user_token, current_task_id, current_task, set_task_mail};
use crate::fs::{make_pipe, OpenFlags, open_file, link, unlink, OSInode};
use alloc::sync::Arc;

#[repr(C)]
#[derive(Debug)]
pub struct Stat {
    pub dev: u64, // ID of device containing file, 文件所在磁盘驱动器号, 暂时不考虑
    pub ino: u64, // inode number, inode 文件所在 inode 编号
    pub mode: StatMode, // file type and mode, 文件类型
    pub nlink: u32, // number of hard links, 硬链接数量，初始为1
    pad: [u64; 7], // unused pad, 无需考虑，为了兼容性设计
}

impl Stat {
    pub fn new() -> Self {
        Stat {
            dev: 0,
            ino: 0,
            mode: StatMode::NULL,
            nlink: 1,
            pad: [0; 7],
        }
    }
}

bitflags! {
    pub struct StatMode: u32 {
        const NULL  = 0;
        const DIR   = 0o040000; // directory
        const FILE  = 0o100000; // ordinary regular file
    }
}

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
        if !file.writable() {
            return -1;
        }
        let file = file.clone();
        // release Task lock manually to avoid deadlock
        drop(inner);
        let (printable, start_pa, end_pa) = virtual_addr_range_printable(token, buf, len);
        if !printable {
            info!("[kernel] buffer overflow in APP {}, in sys_write! v_addr=[{:#x}, {:#x}), p_addr=[{:#x}, {:#x})", current_task_id(), buf as usize, buf as usize + len, start_pa, end_pa);
            return -1 as isize;
        }
        let buffers = translated_byte_buffer(token, buf, len);
        file.write(
            UserBuffer::new(buffers)
        ) as isize
    } else {
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
        if !file.readable() {
            return -1;
        }
        let file = file.clone();
        // release Task lock manually to avoid deadlock
        drop(inner);
        let ret = file.read(
            UserBuffer::new(translated_byte_buffer(token, buf, len))
        ) as isize;
        ret
    } else {
        -1
    }
}

/// 功能：打开一个标准文件，并返回可以访问它的文件描述符
// _dirfd: 仅为了兼容性考虑，本次实验中始终为 AT_FDCWD (-100)。可以忽略。
// path: 描述要打开的文件的文件名
// flags: 描述打开文件的标志
// mode: 仅在创建文件时有用，表示传建文件的访问权限，为了简单，本次实验中可以忽略
pub fn sys_openat(_dirfd: usize, path: *const u8, flags: u32, _mode: u32) -> isize {
    // 有 create 标志但文件存在时，忽略 create 标志，直接打开文件
    // 如果出现了错误则返回 -1，否则返回可以访问给定文件的文件描述符
    // 可能的错误:
    // 1. 文件不存在且无 create 标志
    // 2. 标志非法（低两位为 0x3）
    // 3. 打开文件数量达到上限
    let task = current_task().unwrap();
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(inode) = open_file(
        path.as_str(),
        OpenFlags::from_bits(flags).unwrap()
    ) {
        let mut inner = task.acquire_inner_lock();
        let fd = inner.alloc_fd();
        inner.fd_table[fd] = Some(inode);
        fd as isize
    } else {
        -1
    }
}

/// 功能：当前进程关闭一个文件。
/// 参数：fd 表示要关闭的文件的文件描述符。
/// 返回值：如果成功关闭则返回 0 ，否则返回 -1 。可能的出错原因：传入的文件描述符并不对应一个打开的文件。
/// syscall ID：57
/// 只有当一个管道的所有读端/写端都被关闭之后，管道占用的资源才会被回收，因此我们需要通过关闭文件的系统调用 sys_close 来尽可能早的关闭之后不再用到的读端和写端
/// 可能的错误: 传入的文件描述符 fd 并未被打开或者为保留句柄
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

// Backup 重定向功能
// 在应用执行之前，我们就要对应用进程的文件描述符表进行某种替换
// 以输出为例，我们需要提前打开文件并用这个文件来替换掉应用文件描述符表位置 1 处的标准输出，这就完成了所谓的重定向
/// 功能：将进程中一个已经打开的文件复制一份并分配到一个新的文件描述符中。
/// 参数：fd 表示进程中一个已经打开的文件的文件描述符。
/// 返回值：如果出现了错误则返回 -1，否则能够访问已打开文件的新文件描述符。
/// 可能的错误原因是：传入的 fd 并不对应一个合法的已打开文件。
/// syscall ID：24
pub fn sys_dup(fd: usize) -> isize {
    let task = current_task().unwrap();
    let mut inner = task.acquire_inner_lock();
    // 检查传入 fd 的合法性
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if inner.fd_table[fd].is_none() {
        return -1;
    }
    // 在文件描述符表中分配一个新的文件描述符
    let new_fd = inner.alloc_fd();
    // 保存 fd 指向的已打开文件的一份拷贝即可
    inner.fd_table[new_fd] = Some(Arc::clone(inner.fd_table[fd].as_ref().unwrap()));
    new_fd as isize
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
        let task = current_task().unwrap();
        let mut inner = task.acquire_inner_lock();
        inner.mail_box.write(buffer) as isize
    }
}

// 创建一个文件的一个硬链接
// 硬链接的核心: 多个文件名指向同一个inode
// olddirfd，newdirfd: 仅为了兼容性考虑，本次实验中始终为 AT_FDCWD (-100)，可以忽略
// flags: 仅为了兼容性考虑，本次实验中始终为 0，可以忽略
// oldpath：原有文件路径
// newpath: 新的链接文件路径
// 为了方便，不考虑新文件路径已经存在的情况（属于未定义行为），除非链接同名文件
// 返回值: 果出现了错误则返回 -1，否则返回 0
// 可能的错误: 链接同名文件
pub fn sys_linkat(_olddirfd: i32, oldpath: *const u8, _newdirfd: i32, newpath: *const u8, _flags: u32) -> isize {
    let token = current_user_token();
    let old_path = translated_str(token, oldpath);
    let new_path = translated_str(token, newpath);
    link(&old_path, &new_path)
}

// 取消一个文件路径到文件的链接
// dirfd: 仅为了兼容性考虑，本次实验中始终为 AT_FDCWD (-100)，可以忽略
// flags: 仅为了兼容性考虑，本次实验中始终为 0，可以忽略
// path：文件路径
// 为了方便，不考虑使用 unlink 彻底删除文件的情况
// 返回值：如果出现了错误则返回 -1，否则返回 0。
// 可能的错误: 文件不存在
pub fn sys_unlinkat(_dirfd: i32, path: *const u8, _flags: u32) -> isize {
    let token = current_user_token();
    let path = translated_str(token, path);
    unlink(&path)
}

// 获取文件状态
// fd: 文件描述符
// st: 文件状态结构体
// 如果出现了错误则返回 -1，否则返回 0
// 可能的错误:
//  1. fd 无效
//  2. st 地址非法
pub fn sys_fstat(fd: usize, st: *mut Stat) -> isize {
    let token = current_user_token();
    // check st address
    if !virtual_addr_writable(token, st as usize) {
        return -1 as isize;
    }
    let task = current_task().unwrap();
    let inner = task.acquire_inner_lock();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        unsafe {
            let st_ptr = translated_virtual_ptr(token, st);
            // TODO: 维护并获取file的状态
            if let Some(pa_st) = st_ptr.as_mut() {
                (*pa_st).ino = file.inode_id() as u64;
                (*pa_st).mode = StatMode::FILE;
                (*pa_st).nlink = file.nlink() as u32;
            }
        }
        0
    } else {
        -1
    }
}
