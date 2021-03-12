use super::File;
use alloc::sync::{Arc, Weak};
use spin::Mutex;
use crate::mm::{
    UserBuffer,
};
use crate::task::suspend_current_and_run_next;

// 管道看成一个有 一定缓冲区大小 的 字节[队列]
// 分为读和写两端，需要通过不同的文件描述符来访问
// 管道的缓冲区大小是有限的，一旦整个缓冲区都被填满就不能再继续写入，需要等到读端读取并从队列中弹出一些字符之后才能继续写入

// 将管道的一端（读端或写端）抽象为 Pipe 类型 (而不是管道，是管道的一端！！！)
pub struct Pipe {
    readable: bool,
    writable: bool,
    buffer: Arc<Mutex<PipeRingBuffer>>, // 该管道端所在的管道自身
}

impl Pipe {
    // 从一个已有的管道创建它的读端
    pub fn read_end_with_buffer(buffer: Arc<Mutex<PipeRingBuffer>>) -> Self {
        Self {
            readable: true,
            writable: false, // 不允许向读端写入
            buffer,
        }
    }
    // 从一个已有的管道创建它的写端
    pub fn write_end_with_buffer(buffer: Arc<Mutex<PipeRingBuffer>>) -> Self {
        Self {
            readable: false, // 不允许从写端读取
            writable: true,
            buffer,
        }
    }
}

const RING_BUFFER_SIZE: usize = 32;

#[derive(Copy, Clone, PartialEq)]
enum RingBufferStatus {
    FULL,
    EMPTY,
    NORMAL,
}

// 带有一定大小缓冲区的字节队列
// 每个读端或写端中都保存着所属管道自身的强引用计数，且我们确保这些引用计数只会出现在管道端口 Pipe 结构体中
// 一旦一个管道所有的读端和写端均被关闭，便会导致它们所属管道的引用计数变为 0 ，循环队列缓冲区 arr 所占用的资源被自动回收
// 虽然 PipeRingBuffer 中保存了一个指向写端的引用计数，但是它是一个弱引用，也就不会出现循环引用的情况导致内存泄露
pub struct PipeRingBuffer {
    arr: [u8; RING_BUFFER_SIZE], // 维护一个 循环队列
    head: usize, // 循环队列队头的下标
    tail: usize, // 循环队列队尾的下标
    status: RingBufferStatus, // 缓冲区目前的状态
    write_end: Option<Weak<Pipe>>, // 它的写端的一个弱引用计数(解决循环引用问题), 这是由于在某些情况下需要确认该管道 所有的写端 是否都已经被关闭了
}

impl PipeRingBuffer {
    // 创建一个新的管道
    pub fn new() -> Self {
        Self {
            arr: [0; RING_BUFFER_SIZE],
            head: 0,
            tail: 0,
            status: RingBufferStatus::EMPTY,
            write_end: None,
        }
    }
    pub fn set_write_end(&mut self, write_end: &Arc<Pipe>) {
        self.write_end = Some(Arc::downgrade(write_end));
    }
    pub fn write_byte(&mut self, byte: u8) {
        self.status = RingBufferStatus::NORMAL;
        self.arr[self.tail] = byte; // 写缓冲区
        self.tail = (self.tail + 1) % RING_BUFFER_SIZE;
        // 仅仅通过比较队头和队尾是否相同不能确定循环队列是否为空，因为它既有可能表示队列为空，也有可能表示队列已满
        // 因此我们需要在 read_byte/write_byte 的同时进行状态更新
        if self.tail == self.head {
            self.status = RingBufferStatus::FULL;
        }
    }
    pub fn read_byte(&mut self) -> u8 {
        self.status = RingBufferStatus::NORMAL;
        let c = self.arr[self.head]; // 读缓冲区
        self.head = (self.head + 1) % RING_BUFFER_SIZE;
        if self.head == self.tail {
            self.status = RingBufferStatus::EMPTY;
        }
        c
    }
    // 计算管道中还有 多少个字符 可以读取
    pub fn available_read(&self) -> usize {
        if self.status == RingBufferStatus::EMPTY {
            0 // 队列为空的话直接返回 0
        } else {
            if self.tail > self.head {
                self.tail - self.head
            } else {
                self.tail + RING_BUFFER_SIZE - self.head
            }
        }
    }
    pub fn available_write(&self) -> usize {
        if self.status == RingBufferStatus::FULL {
            0
        } else {
            RING_BUFFER_SIZE - self.available_read()
        }
    }
    // 判断管道的所有写端是否都被关闭了
    pub fn all_write_ends_closed(&self) -> bool {
        // 尝试将管道中保存的写端的弱引用计数升级为强引用计数
        // 如果升级失败的话，说明管道写端的强引用计数为 0 ，也就意味着管道所有写端都被关闭了
        // 从而管道中的数据不会再得到补充
        // 待管道中仅剩的数据被读取完毕之后，管道就可以被销毁了
        self.write_end.as_ref().unwrap().upgrade().is_none()
    }
}

// 创建一个管道并返回它的读端和写端
/// Return (read_end, write_end)
pub fn make_pipe() -> (Arc<Pipe>, Arc<Pipe>) {
    let buffer = Arc::new(Mutex::new(PipeRingBuffer::new()));
    let read_end = Arc::new(
        Pipe::read_end_with_buffer(buffer.clone())
    );
    let write_end = Arc::new(
        Pipe::write_end_with_buffer(buffer.clone())
    );
    // 调用 PipeRingBuffer::set_write_end 在管道中保留它的写端的弱引用计数
    buffer.lock().set_write_end(&write_end);
    (read_end, write_end)
}

impl File for Pipe {
    // 从文件中 最多读取应用缓冲区大小 那么多字符, 这可能超过了队列长度，所以可能需要换出CPU，等待其他应用继续写入管道
    fn read(&self, buf: UserBuffer) -> usize {
        assert_eq!(self.readable, true);
        let mut buf_iter = buf.into_iter();
        let mut read_size = 0usize;
        loop {
            let mut ring_buffer = self.buffer.lock();
            let loop_read = ring_buffer.available_read();
            // 当循环队列中不存在足够字符的时候, 暂时进行任务切换，
            // 等待循环队列中的字符得到补充之后再继续读取
            if loop_read == 0 {
                // 如果管道为空则会检查管道的所有写端是否都已经被关闭，
                // 如果是的话，说明我们已经没有任何字符可以读取了，这时可以直接返回
                if ring_buffer.all_write_ends_closed() {
                    return read_size;
                }
                drop(ring_buffer);
                suspend_current_and_run_next();
                continue;
            }
            // 如果 loop_read 不为 0 ，在这一轮次中管道中就有 loop_read 个字节可以读取
            // read at most loop_read bytes
            for _ in 0..loop_read {
                if let Some(byte_ref) = buf_iter.next() {
                    unsafe { *byte_ref = ring_buffer.read_byte(); }
                    read_size += 1; // 维护实际有多少字节从管道读入应用的缓冲区
                } else {
                    return read_size;
                }
            }
            // 如果这 loop_read 个字节均被读取之后, 还没有填满 应用缓冲区 (而不是管道) 就需要进入循环的下一个轮次
        }
    }
    fn write(&self, buf: UserBuffer) -> usize {
        assert_eq!(self.writable, true);
        let mut buf_iter = buf.into_iter();
        let mut write_size = 0usize;
        loop {
            let mut ring_buffer = self.buffer.lock();
            let loop_write = ring_buffer.available_write();
            // 检查队列是否已满，满的话就停下来，等待其他进程读取管道
            if loop_write == 0 {
                drop(ring_buffer);
                suspend_current_and_run_next();
                continue;
            }
            // write at most loop_write bytes
            for _ in 0..loop_write {
                if let Some(byte_ref) = buf_iter.next() {
                    ring_buffer.write_byte(unsafe { *byte_ref });
                    write_size += 1;
                } else {
                    return write_size;
                }
            }
        }
    }
}
