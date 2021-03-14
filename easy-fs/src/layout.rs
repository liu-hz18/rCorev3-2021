// 磁盘布局及磁盘上数据结构
use core::fmt::{Debug, Formatter, Result};
use super::{
    BLOCK_SZ,
    BlockDevice,
    get_block_cache,
};
use alloc::sync::Arc;
use alloc::vec::Vec;

const EFS_MAGIC: u32 = 0x3b800001;
const INODE_DIRECT_COUNT: usize = 28;
const NAME_LENGTH_LIMIT: usize = 27;
const INODE_INDIRECT1_COUNT: usize = BLOCK_SZ / 4;
const INODE_INDIRECT2_COUNT: usize = INODE_INDIRECT1_COUNT * INODE_INDIRECT1_COUNT;
const DIRECT_BOUND: usize = INODE_DIRECT_COUNT;
const INDIRECT1_BOUND: usize = DIRECT_BOUND + INODE_INDIRECT1_COUNT;
#[allow(unused)]
const INDIRECT2_BOUND: usize = INDIRECT1_BOUND + INODE_INDIRECT2_COUNT;

// 在 easy-fs 磁盘布局中，按照块编号从小到大可以分成 5 个连续区域
// 最开始的区域长度为一个块，其内容是 easy-fs 超级块 (Super Block)，超级块内以魔数的形式提供了文件系统合法性检查功能，同时还可以定位其他连续区域的位置
// 接下来的一个区域是一个索引节点位图, 长度为若干个块。它记录了后面的索引节点区域中有哪些索引节点已经被分配出去使用了，而哪些还尚未被分配出去
// 接下来的一个区域是索引节点区域，长度为若干个块。其中的每个块都存储了若干个索引节点
// 接下来的一个区域是一个数据块位图，长度为若干个块。它记录了后面的数据块区域中有哪些数据块已经被分配出去使用了，而哪些还尚未被分配出去。
// 最后的一个区域则是数据块区域，顾名思义，其中的每一个块的职能都是作为一个数据块实际保存文件或目录中的数据。

#[repr(C)]
pub struct SuperBlock {
    magic: u32, // 用于文件系统合法性验证的魔数
    pub total_blocks: u32, // 给出文件系统的总块数, 并不等同于所在磁盘的总块数，因为文件系统很可能并没有占据整个磁盘
    pub inode_bitmap_blocks: u32, // 四个连续区域的长度各为多少个块
    pub inode_area_blocks: u32,
    pub data_bitmap_blocks: u32,
    pub data_area_blocks: u32,
}

impl Debug for SuperBlock {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        f.debug_struct("SuperBlock")
            .field("total_blocks", &self.total_blocks)
            .field("inode_bitmap_blocks", &self.inode_bitmap_blocks)
            .field("inode_area_blocks", &self.inode_area_blocks)
            .field("data_bitmap_blocks", &self.data_bitmap_blocks)
            .field("data_area_blocks", &self.data_area_blocks)
            .finish()
    }
}

impl SuperBlock {
    pub fn initialize(
        &mut self,
        total_blocks: u32, // 它们的划分是更上层的磁盘块管理器需要完成的工作
        inode_bitmap_blocks: u32,
        inode_area_blocks: u32,
        data_bitmap_blocks: u32,
        data_area_blocks: u32,
    ) {
        *self = Self {
            magic: EFS_MAGIC,
            total_blocks,
            inode_bitmap_blocks,
            inode_area_blocks,
            data_bitmap_blocks,
            data_area_blocks,
        }
    }
    // 通过魔数判断超级块所在的 文件系统 是否合法
    pub fn is_valid(&self) -> bool {
        self.magic == EFS_MAGIC
    }
}

#[derive(PartialEq)]
pub enum DiskInodeType {
    File,
    Directory,
}

type IndirectBlock = [u32; BLOCK_SZ / 4];
// 每个保存内容的 数据块 都只是一个字节数组
type DataBlock = [u8; BLOCK_SZ];

// Inode: 索引节点
// 在 inode 中不仅包含了我们通过 stat 工具能够看到的文件/目录的元数据（大小/访问权限/类型等信息），
// 还包含它到那些实际保存文件/目录数据的数据块（位于最后的数据块区域中）的索引信息，从而能够找到文件/目录的数据被保存在哪里
#[repr(C)]
/// Only support level-1 indirect now, **indirect2** field is always 0.
/// 每个文件/目录在磁盘上均以一个 DiskInode 的形式存储
/// 将 DiskInode 的大小设置为 128 字节，每个块正好能够容纳 4 个 DiskInode
pub struct DiskInode {
    // 文件/目录的元数据
    pub size: u32, // 文件/目录内容的字节数
    // 当取值为 28 的时候，通过直接索引可以找到 14KiB 的内容
    pub direct: [u32; INODE_DIRECT_COUNT], // 直接索引, direct 数组中最多可以指向 INODE_DIRECT_COUNT 个数据块
    pub indirect1: u32, // 一级间接索引. 指向一个位于数据块区域中的一级索引块. 最多能够索引 512/4=128 个数据块, 对应 64KiB 的内容 
    pub indirect2: u32, // 二级间接索引. 指向一个位于数据块区域中的二级索引块. 每个 u32 指向一个不同的一级索引块，这些一级索引块也位于数据块区域中. 最多能够索引 128×64KiB=8MiB 的内容
    type_: DiskInodeType, // 索引节点的类型 DiskInodeType ，目前仅支持文件 File 和目录 Directory 两种类型
}

impl DiskInode {
    /// indirect1 and indirect2 block are allocated only when they are needed.
    pub fn initialize(&mut self, type_: DiskInodeType) {
        // 初始化之后文件/目录的 size 均为 0 ，此时并不会索引到任何数据块
        self.size = 0;
        self.direct.iter_mut().for_each(|v| *v = 0);
        // indirect1/2 均被初始化为 0 。因为最开始文件内容的大小为 0 字节，并不会用到一级/二级索引
        self.indirect1 = 0; // 完全按需分配一级/二级索引块
        self.indirect2 = 0;
        self.type_ = type_;
    }
    // 用来确认 DiskInode 的类型为目录
    pub fn is_dir(&self) -> bool {
        self.type_ == DiskInodeType::Directory
    }
    #[allow(unused)]
    pub fn is_file(&self) -> bool {
        self.type_ == DiskInodeType::File
    }
    /// Return block number correspond to size.
    pub fn data_blocks(&self) -> u32 {
        Self::_data_blocks(self.size)
    }
    // 为了容纳自身 size 字节的内容需要多少个数据块
    // 用 size 除以每个块的大小 BLOCK_SZ 并向上取整
    fn _data_blocks(size: u32) -> u32 {
        (size + BLOCK_SZ as u32 - 1) / BLOCK_SZ as u32
    }
    /// Return number of blocks needed include indirect1/2.
    // 不仅包含数据块，还需要统计索引块
    pub fn total_blocks(size: u32) -> u32 {
        let data_blocks = Self::_data_blocks(size) as usize;
        let mut total = data_blocks as usize;
        // indirect1
        if data_blocks > INODE_DIRECT_COUNT {
            total += 1;
        }
        // indirect2
        if data_blocks > INDIRECT1_BOUND {
            total += 1;
            // sub indirect1
            total += (data_blocks - INDIRECT1_BOUND + INODE_INDIRECT1_COUNT - 1) / INODE_INDIRECT1_COUNT;
        }
        total as u32
    }
    // 将一个 DiskInode 的 size 扩容到 new_size 需要额外多少个数据和索引块
    pub fn blocks_num_needed(&self, new_size: u32) -> u32 {
        assert!(new_size >= self.size);
        Self::total_blocks(new_size) - Self::total_blocks(self.size)
    }
    // 数据块索引功能
    // 从索引中查到它自身用于保存文件内容的第 block_id 个数据块的 块编号
    pub fn get_block_id(&self, inner_id: u32, block_device: &Arc<dyn BlockDevice>) -> u32 {
        let inner_id = inner_id as usize;
        if inner_id < INODE_DIRECT_COUNT {
            self.direct[inner_id]
        } else if inner_id < INDIRECT1_BOUND {
            get_block_cache(self.indirect1 as usize, Arc::clone(block_device))
                .lock()
                .read(0, |indirect_block: &IndirectBlock| {
                    indirect_block[inner_id - INODE_DIRECT_COUNT]
                })
        } else {
            let last = inner_id - INDIRECT1_BOUND;
            let indirect1 = get_block_cache(
                self.indirect2 as usize,
                Arc::clone(block_device)
            )
            .lock()
            .read(0, |indirect2: &IndirectBlock| {
                indirect2[last / INODE_INDIRECT1_COUNT]
            });
            get_block_cache(
                indirect1 as usize,
                Arc::clone(block_device)
            )
            .lock()
            .read(0, |indirect1: &IndirectBlock| {
                indirect1[last % INODE_INDIRECT1_COUNT]
            })
        }
    }
    // 逐步扩充容量
    pub fn increase_size(
        &mut self,
        new_size: u32, // 容量扩充之后的文件大小
        new_blocks: Vec<u32>, // 保存了本次容量扩充所需 块编号 的向量, 由上层的磁盘块管理器负责分配的
        block_device: &Arc<dyn BlockDevice>,
    ) {
        // 按照直接索引、一级索引再到二级索引的顺序进行扩充
        let mut current_blocks = self.data_blocks();
        self.size = new_size;
        let mut total_blocks = self.data_blocks();
        let mut new_blocks = new_blocks.into_iter();
        // fill direct
        while current_blocks < total_blocks.min(INODE_DIRECT_COUNT as u32) {
            self.direct[current_blocks as usize] = new_blocks.next().unwrap();
            current_blocks += 1;
        }
        // alloc indirect1
        if total_blocks > INODE_DIRECT_COUNT as u32{
            if current_blocks == INODE_DIRECT_COUNT as u32 {
                self.indirect1 = new_blocks.next().unwrap();
            }
            current_blocks -= INODE_DIRECT_COUNT as u32;
            total_blocks -= INODE_DIRECT_COUNT as u32;
        } else {
            return;
        }
        // fill indirect1
        get_block_cache(
            self.indirect1 as usize,
            Arc::clone(block_device)
        )
        .lock()
        .modify(0, |indirect1: &mut IndirectBlock| {
            while current_blocks < total_blocks.min(INODE_INDIRECT1_COUNT as u32) {
                indirect1[current_blocks as usize] = new_blocks.next().unwrap();
                current_blocks += 1;
            }
        });
        // alloc indirect2
        if total_blocks > INODE_INDIRECT1_COUNT as u32 {
            if current_blocks == INODE_INDIRECT1_COUNT as u32 {
                self.indirect2 = new_blocks.next().unwrap();
            }
            current_blocks -= INODE_INDIRECT1_COUNT as u32;
            total_blocks -= INODE_INDIRECT1_COUNT as u32;
        } else {
            return;
        }
        // fill indirect2 from (a0, b0) -> (a1, b1)
        let mut a0 = current_blocks as usize / INODE_INDIRECT1_COUNT;
        let mut b0 = current_blocks as usize % INODE_INDIRECT1_COUNT;
        let a1 = total_blocks as usize / INODE_INDIRECT1_COUNT;
        let b1 = total_blocks as usize % INODE_INDIRECT1_COUNT;
        // alloc low-level indirect1
        get_block_cache(
            self.indirect2 as usize,
            Arc::clone(block_device)
        )
        .lock()
        .modify(0, |indirect2: &mut IndirectBlock| {
            while (a0 < a1) || (a0 == a1 && b0 < b1) {
                if b0 == 0 {
                    indirect2[a0] = new_blocks.next().unwrap();
                }
                // fill current
                get_block_cache(
                    indirect2[a0] as usize,
                    Arc::clone(block_device)
                )
                .lock()
                .modify(0, |indirect1: &mut IndirectBlock| {
                    indirect1[b0] = new_blocks.next().unwrap();
                });
                // move to next
                b0 += 1;
                if b0 == INODE_INDIRECT1_COUNT {
                    b0 = 0;
                    a0 += 1;
                }
            } 
        });
    }
    
    /*
    pub fn clear_size(&mut self, block_device: &Arc<dyn BlockDevice>) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let blocks = self.blocks() as usize;
        self.size = 0;
        for i in 0..blocks.min(INODE_DIRECT_COUNT) {
            v.push(self.direct[i]);
            self.direct[i] = 0;
        }
        if blocks > INODE_DIRECT_COUNT {
            get_block_cache(
                self.indirect1 as usize,
                Arc::clone(block_device),
            )
            .lock()
            .modify(0, |indirect_block: &mut IndirectBlock| {
                for i in 0..blocks - INODE_DIRECT_COUNT {
                    v.push(indirect_block[i]);
                    indirect_block[i] = 0;
                }
            });
        }
        v
    }
    */

    // 清空文件的内容并回收所有数据和索引块
    /// Clear size to zero and return blocks that should be deallocated.
    ///
    /// We will clear the block contents to zero later.
    pub fn clear_size(&mut self, block_device: &Arc<dyn BlockDevice>) -> Vec<u32> {
        // 回收的所有块的编号保存在一个向量中返回给磁盘块管理器
        let mut v: Vec<u32> = Vec::new();
        let mut data_blocks = self.data_blocks() as usize;
        self.size = 0;
        let mut current_blocks = 0usize;
        // direct
        while current_blocks < data_blocks.min(INODE_DIRECT_COUNT) {
            v.push(self.direct[current_blocks]);
            self.direct[current_blocks] = 0;
            current_blocks += 1;
        }
        // indirect1 block
        if data_blocks > INODE_DIRECT_COUNT {
            v.push(self.indirect1);
            data_blocks -= INODE_DIRECT_COUNT;
            current_blocks = 0;
        } else {
            return v;
        }
        // indirect1
        get_block_cache(
            self.indirect1 as usize,
            Arc::clone(block_device),
        )
        .lock()
        .modify(0, |indirect1: &mut IndirectBlock| {
            while current_blocks < data_blocks.min(INODE_INDIRECT1_COUNT) {
                v.push(indirect1[current_blocks]);
                //indirect1[current_blocks] = 0;
                current_blocks += 1;
            }
        });
        self.indirect1 = 0;
        // indirect2 block
        if data_blocks > INODE_INDIRECT1_COUNT {
            v.push(self.indirect2);
            data_blocks -= INODE_INDIRECT1_COUNT;
        } else {
            return v;
        }
        // indirect2
        assert!(data_blocks <= INODE_INDIRECT2_COUNT);
        let a1 = data_blocks / INODE_INDIRECT1_COUNT;
        let b1 = data_blocks % INODE_INDIRECT1_COUNT;
        get_block_cache(
            self.indirect2 as usize,
            Arc::clone(block_device),
        )
        .lock()
        .modify(0, |indirect2: &mut IndirectBlock| {
            // full indirect1 blocks
            for i in 0..a1 {
                v.push(indirect2[i]);
                get_block_cache(
                    indirect2[i] as usize,
                    Arc::clone(block_device),
                )
                .lock()
                .modify(0, |indirect1: &mut IndirectBlock| {
                    for j in 0..INODE_INDIRECT1_COUNT {
                        v.push(indirect1[j]);
                        //indirect1[j] = 0;
                    }
                });
                //indirect2[i] = 0;
            }
            // last indirect1 block
            if b1 > 0 {
                v.push(indirect2[a1]);
                get_block_cache(
                    indirect2[a1] as usize,
                    Arc::clone(block_device),
                )
                .lock()
                .modify(0, |indirect1: &mut IndirectBlock| {
                    for j in 0..b1 {
                        v.push(indirect1[j]);
                        //indirect1[j] = 0;
                    }
                });
                //indirect2[a1] = 0;
            }
        });
        self.indirect2 = 0;
        v
    }
    // 通过 DiskInode 来读写它索引的那些数据块中的数据
    // 每次我们都是选取其中的一段连续区间进行操作
    // 将文件内容从 offset 字节开始的部分读到内存中的缓冲区 buf 中，并返回实际读到的字节数
    // 如果文件剩下的内容还足够多，那么缓冲区会被填满；不然的话文件剩下的全部内容都会被读到缓冲区中
    pub fn read_at(
        &self,
        offset: usize,
        buf: &mut [u8],
        block_device: &Arc<dyn BlockDevice>,
    ) -> usize {
        let mut start = offset;
        let end = (offset + buf.len()).min(self.size as usize);
        // 要读取的内容超出了文件的范围那么直接返回 0 表示读取不到任何内容
        if start >= end {
            return 0;
        }
        let mut start_block = start / BLOCK_SZ;
        let mut read_size = 0usize;
        loop {
            // calculate end of current block
            let mut end_current_block = (start / BLOCK_SZ + 1) * BLOCK_SZ;
            end_current_block = end_current_block.min(end);
            // read and update read size
            let block_read_size = end_current_block - start;
            let dst = &mut buf[read_size..read_size + block_read_size];
            get_block_cache(
                self.get_block_id(start_block as u32, block_device) as usize,
                Arc::clone(block_device),
            )
            .lock()
            .read(0, |data_block: &DataBlock| {
                let src = &data_block[start % BLOCK_SZ..start % BLOCK_SZ + block_read_size];
                dst.copy_from_slice(src);
            });
            read_size += block_read_size;
            // move to next block
            if end_current_block == end { break; }
            start_block += 1;
            start = end_current_block;
        }
        read_size
    }
    /// File size must be adjusted before.
    // 不会出现失败的情况，传入的整个缓冲区的数据都必定会被写入到文件中
    // 当从 offset 开始的区间超出了文件范围的时候，就需要调用者在调用 write_at 之前提前调用 increase_size 将文件大小扩充到区间的右端保证写入的完整性
    pub fn write_at(
        &mut self,
        offset: usize,
        buf: &[u8],
        block_device: &Arc<dyn BlockDevice>,
    ) -> usize {
        let mut start = offset;
        let end = (offset + buf.len()).min(self.size as usize);
        assert!(start <= end);
        let mut start_block = start / BLOCK_SZ;
        let mut write_size = 0usize;
        loop {
            // calculate end of current block
            let mut end_current_block = (start / BLOCK_SZ + 1) * BLOCK_SZ;
            end_current_block = end_current_block.min(end);
            // write and update write size
            let block_write_size = end_current_block - start;
            get_block_cache(
                self.get_block_id(start_block as u32, block_device) as usize,
                Arc::clone(block_device)
            )
            .lock()
            .modify(0, |data_block: &mut DataBlock| {
                let src = &buf[write_size..write_size + block_write_size];
                let dst = &mut data_block[start % BLOCK_SZ..start % BLOCK_SZ + block_write_size];
                dst.copy_from_slice(src);
            });
            write_size += block_write_size;
            // move to next block
            if end_current_block == end { break; }
            start_block += 1;
            start = end_current_block;
        }
        write_size
    }
}

// 目录项相当于目录树结构上的孩子指针，我们需要通过它来一级一级的找到实际要访问的文件或目录
#[repr(C)]
pub struct DirEntry {
    name: [u8; NAME_LENGTH_LIMIT + 1],
    inode_number: u32,
}

pub const DIRENT_SZ: usize = 32;

// 自身占据空间 32 字节，每个数据块可以存储 16 个目录项
//pub type DirentBlock = [DirEntry; BLOCK_SZ / DIRENT_SZ];
pub type DirentBytes = [u8; DIRENT_SZ];

impl DirEntry {
    // 一个合法的目录项
    pub fn new(name: &str, inode_number: u32) -> Self {
        let mut bytes = [0u8; NAME_LENGTH_LIMIT + 1];
        &mut bytes[..name.len()].copy_from_slice(name.as_bytes());
        Self {
            name: bytes,
            inode_number,
        }
    }
    // 将目录项转化为缓冲区（即字节切片）的形式来符合 read/write_at 接口的要求
    pub fn into_bytes(&self) -> &DirentBytes {
        unsafe {
            &*(self as *const Self as usize as *const DirentBytes)
        }
    }
    pub fn from_bytes(bytes: &DirentBytes) -> &Self {
        unsafe { &*(bytes.as_ptr() as usize as *const Self) }
    }
    #[allow(unused)]
    pub fn from_bytes_mut(bytes: &mut DirentBytes) -> &mut Self {
        unsafe {
            &mut *(bytes.as_mut_ptr() as usize as *mut Self)
        }
    }
    pub fn name(&self) -> &str {
        let len = (0usize..).find(|i| self.name[*i] == 0).unwrap();
        core::str::from_utf8(&self.name[..len]).unwrap()
    }
    pub fn inode_number(&self) -> u32 {
        self.inode_number
    }
}
