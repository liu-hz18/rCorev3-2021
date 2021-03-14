use core::any::Any;

// 最底层: 块设备的抽象接口

// 块设备仅支持以块为单位进行随机读写，由此才有了这两个抽象方法。
// 由库的使用者提供并接入到 easy-fs 库
pub trait BlockDevice : Send + Sync + Any {
    // 将编号为 block_id 的块从磁盘读入内存中的缓冲区 buf
    fn read_block(&self, block_id: usize, buf: &mut [u8]);
    // 内存中的缓冲区 buf 中的数据写入磁盘编号为 block_id 的块
    fn write_block(&self, block_id: usize, buf: &[u8]);
}
