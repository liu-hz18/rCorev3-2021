use super::{
    BLOCK_SZ,
    BlockDevice,
};
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use lazy_static::*;
use spin::Mutex;

// 自下而上的第二层: 块缓存层

pub struct BlockCache {
    cache: [u8; BLOCK_SZ], // 一个 512 字节的数组，表示位于内存中的缓冲区
    block_id: usize, // 记录了这个块缓存来自于磁盘中的块的编号
    block_device: Arc<dyn BlockDevice>, // 保留一个底层块设备的引用使得可以和它打交道
    modified: bool, // 记录自从这个块缓存从磁盘载入内存(cache)之后，它有没有被修改过
}

/// 每当要对一个磁盘块进行读写的时候，都通过 read_block 将块数据读取到一个 临时 创建的缓冲区，并在进行一些操作之后（可选地）将缓冲区的内容写回到磁盘块
/// 从性能上考虑，我们需要尽可能降低真正块读写（即 read/write_block ）的次数，因为每一次调用它们都会产生大量开销
/// 要做到这一点，关键就在于对于块读写操作进行 合并
/// 当我们要读写一个块的时候，首先就是去全局管理器中查看这个块是否已被缓存到内存中的缓冲区中。这样，在一段连续时间内对于一个块进行的所有操作均是在同一个固定的缓冲区中进行的，这解决了同步性问题
/// 通过 read/write_block 真正进行块读写的时机完全交给全局管理器处理，我们在编程时无需操心。全局管理器仅会在必要的时机分别发起一次真正的块读写，尽可能将更多的块操作合并起来
impl BlockCache {
    /// Load a new BlockCache from disk.
    // 创建一个 BlockCache 的时候，这将触发一次 read_block 将一个块上的数据从磁盘读到缓冲区 cache
    pub fn new(
        block_id: usize, 
        block_device: Arc<dyn BlockDevice>
    ) -> Self {
        let mut cache = [0u8; BLOCK_SZ];
        block_device.read_block(block_id, &mut cache);
        Self {
            cache,
            block_id,
            block_device,
            modified: false,
        }
    }

    // 一旦缓冲区已经存在于内存中，CPU 就可以直接访问存储在它上面的磁盘数据结构
    // 得到一个 BlockCache 内部的缓冲区一个指定偏移量 offset 的字节地址
    fn addr_of_offset(&self, offset: usize) -> usize {
        &self.cache[offset] as *const _ as usize
    }

    // 获取缓冲区中的位于偏移量 offset 的一个类型为 T 的磁盘上数据结构的不可变引用
    pub fn get_ref<T>(&self, offset: usize) -> &T where T: Sized {
        // 获取类型 T 的大小并确认该数据结构被整个包含在磁盘块及其缓冲区之内
        let type_size = core::mem::size_of::<T>();
        assert!(offset + type_size <= BLOCK_SZ);
        let addr = self.addr_of_offset(offset);
        unsafe { &*(addr as *const T) } 
    }

    // 获取磁盘上数据结构的可变引用
    pub fn get_mut<T>(&mut self, offset: usize) -> &mut T where T: Sized {
        let type_size = core::mem::size_of::<T>();
        assert!(offset + type_size <= BLOCK_SZ);
        // 标记为 true 表示该缓冲区已经被修改，之后需要将数据写回磁盘块才能真正将修改同步到磁盘
        self.modified = true;
        let addr = self.addr_of_offset(offset);
        unsafe { &mut *(addr as *mut T) }
    }

    // 将 get_ref/get_mut 进一步封装为更为易用的形式
    pub fn read<T, V>(&self, offset: usize, f: impl FnOnce(&T) -> V) -> V {
        // 进行传入的闭包 f 中所定义的操作
        f(self.get_ref(offset))
    }

    pub fn modify<T, V>(&mut self, offset:usize, f: impl FnOnce(&mut T) -> V) -> V {
        f(self.get_mut(offset))
    }

    // 在我们简单的实现中，sync 仅会在 BlockCache 被 drop 时才会被调用
    // 但是linux中，sync 并不是只有在 drop 的时候才会被调用
    pub fn sync(&mut self) {
        if self.modified {
            self.modified = false;
            self.block_device.write_block(self.block_id, &self.cache);
        }
    }
}

// RAII: 管理着一个缓冲区的生命周期。当 BlockCache 的生命周期结束之后缓冲区也会被从内存中回收，
//       这个时候 modified 标记将会决定数据是否需要写回磁盘
impl Drop for BlockCache {
    fn drop(&mut self) {
        self.sync()
    }
}

const BLOCK_CACHE_SIZE: usize = 16;

// 块缓存全局管理器
// 为了避免在块缓存上浪费过多内存，我们希望内存中同时只能驻留 有限个磁盘块的缓冲区
pub struct BlockCacheManager {
    // 块编号和块缓存的二元组
    queue: VecDeque<(usize, Arc<Mutex<BlockCache>>)>,
}

// 功能:
// 当我们要对一个磁盘块进行读写从而需要获取它的缓冲区的时候，首先看它是否已经被载入到内存中了，
// 如果已经被载入的话则直接返回，否则需要读取磁盘块的数据到内存中
// 如果内存中驻留的磁盘块缓冲区的数量已满，则需要遵循某种缓存替换算法将某个块的缓冲区从内存中移除，再将刚刚请求的块的缓冲区加入到内存中
// 这里使用一种类 FIFO 的简单缓存替换算法
impl BlockCacheManager {
    pub fn new() -> Self {
        Self { queue: VecDeque::new() }
    }

    // 从块缓存管理器中获取一个编号为 block_id 的块的块缓存，如果找不到的话会从磁盘读取到内存中，还有可能会发生缓存替换
    pub fn get_block_cache(
        &mut self,
        block_id: usize,
        block_device: Arc<dyn BlockDevice>,
    ) -> Arc<Mutex<BlockCache>> {
        // 遍历整个队列试图找到一个编号相同的块缓存，如果找到了话会将块缓存管理器中保存的块缓存的引用复制一份并返回
        if let Some(pair) = self.queue
            .iter()
            .find(|pair| pair.0 == block_id) {
                Arc::clone(&pair.1)
        } else {
            // 找不到的情况，此时必须将块从磁盘读入内存中的缓冲区
            // substitute
            // 类 FIFO 算法
            // 此时队头对应的块缓存可能仍在使用：判断的标志是其强引用计数 ≥2 ，即除了块缓存管理器保留的一份副本之外，在外面还有若干份副本正在使用
            // 因此，我们的做法是从队头遍历到队尾找到第一个强引用计数恰好为 1 的块缓存并将其替换出去
            if self.queue.len() == BLOCK_CACHE_SIZE {
                // from front to tail
                if let Some((idx, _)) = self.queue
                    .iter()
                    .enumerate()
                    .find(|(_, pair)| Arc::strong_count(&pair.1) == 1) {
                    self.queue.drain(idx..=idx);
                } else {
                    // 要我们的上限 BLOCK_CACHE_SIZE 设置的足够大，超过所有线程同时访问的块总数上限，
                    // 那么 队列已满且其中所有的块缓存都正在使用的情形 永远不会发生
                    // 但是，如果我们的上限设置不足，这里我们就只能 panic
                    panic!("Run out of BlockCache!");
                }
            }
            // 创建一个新的块缓存（会触发 read_block 进行块读取）并加入到队尾，最后返回给请求者
            // load block into mem and push back
            let block_cache = Arc::new(Mutex::new(
                BlockCache::new(block_id, Arc::clone(&block_device))
            ));
            self.queue.push_back((block_id, Arc::clone(&block_cache)));
            block_cache
        }
    }
}

// 创建 BlockCacheManager 的全局实例
lazy_static! {
    pub static ref BLOCK_CACHE_MANAGER: Mutex<BlockCacheManager> = Mutex::new(
        BlockCacheManager::new()
    );
}

// 请求块缓存
// 调用者需要通过 .lock() 获取里层互斥锁 Mutex 才能对最里面的 BlockCache 进行操作
pub fn get_block_cache(
    block_id: usize,
    block_device: Arc<dyn BlockDevice>
) -> Arc<Mutex<BlockCache>> {
    BLOCK_CACHE_MANAGER.lock().get_block_cache(block_id, block_device)
}
