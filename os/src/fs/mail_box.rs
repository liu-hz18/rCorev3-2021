use alloc::vec::Vec;
use alloc::collections::{VecDeque};
use crate::mm::{UserBuffer};

const MAX_PACKET_NUM: usize = 16;

pub struct MailBox {
    pub size: usize, // 栈顶index, 同时标记栈大小
    pub packets: VecDeque<MailPacket>, // 文件描述符表
}

impl MailBox {
    pub fn new() -> Self {
        Self {
            size: 0,
            packets: VecDeque::new(),
        }
    }
    pub fn push(&mut self, packet: MailPacket) {
        self.packets.push_back(packet);
        self.size += 1;
    }
    pub fn write(&mut self, user_buf: UserBuffer) -> isize {
        if self.size >= MAX_PACKET_NUM { // 邮箱已满
            return -1;
        }
        let packet: MailPacket = MailPacket::from_buffer(user_buf);
        if packet.len > 0 { // 长度为0就不push
            self.packets.push_back(packet);
            self.size += 1;
        }
        // info!("[kernel] packet len={}", packet.len as isize);
        packet.len as isize
    }
    pub fn read(&mut self, user_buf: UserBuffer) -> isize {
        if self.size == 0 { // 邮箱空
            return -1;
        }
        if user_buf.len() > 0 {
            if let Some(packet) = self.packets.pop_front() {
                self.size -= 1;
                packet.write_buf(user_buf) as isize
            } else {
                -1
            }
        } else {
            0
        }
    }
}

const PACKET_BUFFER_SIZE: usize = 256;

#[derive(Copy, Clone, PartialEq)]
pub struct MailPacket {
    arr: [u8; PACKET_BUFFER_SIZE], // 报文缓冲区
    len: usize // 报文长度
}

impl MailPacket {
    pub fn new() -> Self {
        Self {
            arr: [0u8; PACKET_BUFFER_SIZE],
            len: 0
        }
    }
    pub fn from_buffer(user_buf: UserBuffer) -> Self {
        let mut packet = MailPacket::new();
        for buffer in user_buf {
            unsafe { packet.arr[packet.len] = *buffer; }
            packet.len += 1;
            if packet.len >= PACKET_BUFFER_SIZE {
                break;
            }
        }
        packet
    }
    // read to buffer
    pub fn write_buf(&self, user_buf: UserBuffer) -> usize {
        let mut buf_iter = user_buf.into_iter();
        let mut write_size = 0usize;
        for _i in 0..self.len {
            if let Some(byte_ref) = buf_iter.next() {
                unsafe { *byte_ref = self.arr[write_size]; }
                write_size += 1;
            } else {
                return write_size;
            }
        }
        return write_size;
    }
}
