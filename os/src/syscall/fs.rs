const FD_STDOUT: usize = 1;
use crate::mm::translated_byte_buffer;
use crate::task::current_user_token;

// use crate::loader::{in_app_user_stack, in_app_data_section};
// use crate::task::current_task_id;

// fn addr_range_valid(buf: usize, len: usize) -> bool {
//     let task_id = current_task_id();
//     (in_app_user_stack(task_id, buf) && in_app_user_stack(task_id, buf+len)) ||
//     (in_app_data_section(task_id, buf) && in_app_data_section(task_id, buf+len))
// }

// 由于内核和应用地址空间的隔离， sys_write 不再能够直接访问位于应用空间中的数据，而需要手动查页表才能知道那些 数据被放置在哪些物理页帧上并进行访问
// 安全检查：sys_write 仅能输出位于程序本身内存空间内的数据，否则报错
pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    match fd {
        // 注意这里我们并没有检查传入参数的安全性，即使会在出错严重的时候 panic，还是会存在安全隐患。
        FD_STDOUT => {
            // check if in user stack
            // check if in .text .data .bss
            // if !addr_range_valid(buf as usize, len) {
            //     println!("[kernel] buffer overflow in APP {}, in sys_write! addr=[{:#x}, {:#x})", current_task_id(), buf as usize, buf as usize + len);
            //     -1 as isize
            // }
            let buffers = translated_byte_buffer(current_user_token(), buf, len);
            for buffer in buffers {
                print!("{}", core::str::from_utf8(buffer).unwrap()); // 尝试将每个字节数组切片转化为字符串 &str 然后输出即可
            }
            len as isize
        },
        _ => {
            println!("[kernel] Unsupported fd in sys_write!");
            return -1 as isize;
        }
    }
}
