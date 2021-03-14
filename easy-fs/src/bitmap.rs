use alloc::sync::Arc;
use super::{
    BlockDevice,
    BLOCK_SZ,
    get_block_cache,
};

// 在 easy-fs 布局中存在两个不同的位图，分别对于索引节点和数据块进行管理

// 每个块大小为 512 字节，即 4096 个比特
// 0 意味着未分配，而 1 则意味着已经分配出去
type BitmapBlock = [u64; 64];

const BLOCK_BITS: usize = BLOCK_SZ * 8;

// 自身是驻留在内存中的
// 可以以组为单位进行操作
pub struct Bitmap {
    start_block_id: usize, // 所在区域的起始块编号
    blocks: usize, // 区域的长度为多少个块
}

// 将比特编号 bit 分解为区域中的块编号 block_pos 、块内的组编号 bits64_pos 以及组内编号 inner_pos 的三元组
/// Return (block_pos, bits64_pos, inner_pos)
fn decomposition(mut bit: usize) -> (usize, usize, usize) {
    let block_pos = bit / BLOCK_BITS;
    bit = bit % BLOCK_BITS;
    (block_pos, bit / 64, bit % 64)
}

impl Bitmap {
    pub fn new(start_block_id: usize, blocks: usize) -> Self {
        Self {
            start_block_id,
            blocks,
        }
    }

    // 分配一个比特
    pub fn alloc(&self, block_device: &Arc<dyn BlockDevice>) -> Option<usize> {
        // 遍历区域中的每个块，再在每个块中以比特组（每组 64 比特）为单位进行遍历
        // 找到一个尚未被全部分配出去的组，最后在里面分配一个比特
        // 它将会返回分配的比特所在的位置，等同于索引节点/数据块的编号
        for block_id in 0..self.blocks {
            let pos = get_block_cache(
                // 传入的块编号是区域起始块编号 start_block_id 加上区域内的块编号 block_id 得到的块设备上的块编号
                block_id + self.start_block_id as usize,
                Arc::clone(block_device),
            // 从缓冲区偏移量为 0 的位置开始将一段连续的数据（数据的长度随具体类型而定）解析为一个 BitmapBlock 并要对该数据结构进行修改
            ).lock().modify(0, |bitmap_block: &mut BitmapBlock| { // 它传入的偏移量 offset 为 0，这是因为整个块上只有一个 BitmapBlock ，它的大小恰好为 512 字节
                // 闭包需要显式声明参数类型为 &mut BitmapBlock
                // 不然的话， BlockCache 的泛型方法 modify/get_mut 无法得知应该用哪个类型来解析块上的数据
                
                // 尝试在 bitmap_block 中找到一个空闲的比特并返回其位置，如果不存在的话则返回 None
                if let Some((bits64_pos, inner_pos)) = bitmap_block
                    .iter()
                    .enumerate() // 遍历每 64 个比特构成的组（一个 u64 ），如果它并没有达到 u64::MAX
                    .find(|(_, bits64)| **bits64 != u64::MAX)
                    .map(|(bits64_pos, bits64)| {
                        (bits64_pos, bits64.trailing_ones() as usize) // 则通过 u64::trailing_ones 找到最低的一个 0 并置为 1
                    }) {
                    // 如果能够找到的话，比特组的编号将保存在变量 bits64_pos 中
                    // 而分配的比特在组内的位置将保存在变量 inner_pos 中
                    // modify cache
                    bitmap_block[bits64_pos] |= 1u64 << inner_pos;
                    Some(block_id * BLOCK_BITS + bits64_pos * 64 + inner_pos as usize)
                } else {
                    None
                }
            });
            // 提前返回: 一旦在某个块中找到一个空闲的比特并成功分配，就不再考虑后续的块
            if pos.is_some() {
                return pos;
            }
        }
        None
    }

    // 回收一个比特
    pub fn dealloc(&self, block_device: &Arc<dyn BlockDevice>, bit: usize) {
        let (block_pos, bits64_pos, inner_pos) = decomposition(bit);
        get_block_cache(
            block_pos + self.start_block_id,
            Arc::clone(block_device)
        ).lock().modify(0, |bitmap_block: &mut BitmapBlock| {
            assert!(bitmap_block[bits64_pos] & (1u64 << inner_pos) > 0);
            // 将其清零即可
            bitmap_block[bits64_pos] -= 1u64 << inner_pos;
        });
    }

    pub fn maximum(&self) -> usize {
        self.blocks * BLOCK_BITS
    }
}