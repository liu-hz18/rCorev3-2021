use crate::mm::{translated_byte_buffer, virtual_addr_range_printable};
use crate::task::{current_user_token, current_task_id, suspend_current_and_run_next};
use crate::sbi::console_getchar;

const FD_STDIN: usize = 0;
const FD_STDOUT: usize = 1;

// 由于内核和应用地址空间的隔离， sys_write 不再能够直接访问位于应用空间中的数据，而需要手动查页表才能知道那些 数据被放置在哪些物理页帧上并进行访问
// 安全检查：sys_write 仅能输出位于程序本身内存空间内的数据，否则报错
pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    match fd {
        // 注意这里我们并没有检查传入参数的安全性，即使会在出错严重的时候 panic，还是会存在安全隐患。
        FD_STDOUT => {
            let (printable, start_pa, end_pa) = virtual_addr_range_printable(current_user_token(), buf, len);
            if !printable {
                println!("[kernel] buffer overflow in APP {}, in sys_write! v_addr=[{:#x}, {:#x}), p_addr=[{:#x}, {:#x})", current_task_id(), buf as usize, buf as usize + len, start_pa, end_pa);
                return -1 as isize;
            }
            let buffers = translated_byte_buffer(current_user_token(), buf, len);
            for buffer in buffers {
                print!("{}", core::str::from_utf8(buffer).unwrap()); // 尝试将每个字节数组切片转化为字符串 &str 然后输出即可
            }
            return len as isize;
        },
        _ => {
            println!("[kernel] Unsupported fd in sys_write!");
            return -1 as isize;
        }
    }
}

pub fn sys_read(fd: usize, buf: *const u8, len: usize) -> isize {
    match fd {
        FD_STDIN => {
            // 单次读入的长度限制为 1，即每次只能读入一个字符
            assert_eq!(len, 1, "Only support len = 1 in sys_read!");
            let mut c: usize;
            loop {
                // 如果返回 0 的话说明还没有输入，我们调用 suspend_current_and_run_next 暂时切换到其他进程
                c = console_getchar();
                if c == 0 {
                    suspend_current_and_run_next();
                    continue;
                } else {
                    break;
                }
            }
            // 获取到输入之后，我们退出循环并手动查页表将输入的字符正确的写入到应用地址空间
            let ch = c as u8;
            let mut buffers = translated_byte_buffer(current_user_token(), buf, len);
            unsafe { buffers[0].as_mut_ptr().write_volatile(ch); }
            1
        }
        _ => {
            panic!("Unsupported fd in sys_read!");
        }
    }
}
