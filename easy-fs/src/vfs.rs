use super::{
    BlockDevice,
    DiskInode,
    DiskInodeType,
    DirEntry,
    DirentBytes,
    EasyFileSystem,
    DIRENT_SZ,
    get_block_cache,
};
use alloc::sync::Arc;
use alloc::string::String;
use alloc::vec::Vec;
use spin::{Mutex, MutexGuard};

// DiskInode 放在磁盘块中比较固定的位置，而 Inode 是放在内存中的
pub struct Inode {
    inode_id: usize,
    // block_id 和 block_offset 记录该 Inode 对应的 DiskInode 保存在磁盘上的具体位置
    block_id: usize,
    block_offset: usize,
    fs: Arc<Mutex<EasyFileSystem>>,
    block_device: Arc<dyn BlockDevice>,
}

impl Inode {
    pub fn new(
        inode_id: u32,
        fs: Arc<Mutex<EasyFileSystem>>,
        block_device: Arc<dyn BlockDevice>,
    ) -> Self {
        let (block_id, block_offset) = fs.lock().get_disk_inode_pos(inode_id);
        Self {
            inode_id: inode_id as usize,
            block_id: block_id as usize,
            block_offset,
            fs,
            block_device,
        }
    }

    // 简化对于 Inode 对应的磁盘上的 DiskInode 的访问流程
    fn read_disk_inode<V>(&self, f: impl FnOnce(&DiskInode) -> V) -> V {
        get_block_cache(
            self.block_id,
            Arc::clone(&self.block_device)
        ).lock().read(self.block_offset, f)
    }

    fn modify_disk_inode<V>(&self, f: impl FnOnce(&mut DiskInode) -> V) -> V {
        get_block_cache(
            self.block_id,
            Arc::clone(&self.block_device)
        ).lock().modify(self.block_offset, f)
    }

    /*
    fn get_disk_inode(&self, fs: &mut MutexGuard<EasyFileSystem>) -> Dirty<DiskInode> {
        fs.get_disk_inode(self.inode_id)
    }
    */

    // 尝试从根目录的 DiskInode 上找到要索引的文件名对应的 inode 编号
    fn find_inode_id(
        &self,
        name: &str,
        disk_inode: &DiskInode,
    ) -> Option<u32> {
        // assert it is a directory
        assert!(disk_inode.is_dir());
        let file_count = (disk_inode.size as usize) / DIRENT_SZ;
        let mut dirent_space: DirentBytes = Default::default();
        for i in 0..file_count {
            assert_eq!(
                disk_inode.read_at(
                    DIRENT_SZ * i,
                    &mut dirent_space,
                    &self.block_device,
                ),
                DIRENT_SZ,
            );
            let dirent = DirEntry::from_bytes(&dirent_space);
            if dirent.name() == name {
                return Some(dirent.inode_number() as u32);
            }
        }
        None
    }

    // find 方法只会被根目录 Inode 调用，文件系统中其他文件的 Inode 不会调用这个方法
    pub fn find(&self, name: &str) -> Option<Arc<Inode>> {
        let _ = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            self.find_inode_id(name, disk_inode)
            .map(|inode_id| {
                // 根据查到 inode 编号对应生成一个 Inode 用于后续对文件的访问
                Arc::new(Self::new(
                    inode_id,
                    self.fs.clone(),
                    self.block_device.clone(),
                ))
            })
        })
    }

    pub fn get_inode_id(&self) -> usize {
        self.inode_id
    }

    fn increase_size(
        &self,
        new_size: u32,
        disk_inode: &mut DiskInode,
        fs: &mut MutexGuard<EasyFileSystem>,
    ) {
        if new_size < disk_inode.size {
            return;
        }
        let blocks_needed = disk_inode.blocks_num_needed(new_size);
        let mut v: Vec<u32> = Vec::new();
        for _ in 0..blocks_needed {
            v.push(fs.alloc_data());
        }
        disk_inode.increase_size(new_size, v, &self.block_device);
    }

    // 在根目录下创建一个文件，该方法只有根目录的 Inode 会调用
    pub fn create(&self, name: &str) -> Option<Arc<Inode>> {
        let mut fs = self.fs.lock();
        // 检查文件是否已经在根目录下，如果找到的话返回 None
        if self.modify_disk_inode(|root_inode| {
            // assert it is a directory
            assert!(root_inode.is_dir());
            // has the file been created?
            self.find_inode_id(name, root_inode)
        }).is_some() {
            return None;
        }
        // 为待创建文件分配一个新的 inode 并进行初始化
        // create a new file
        // alloc a inode with an indirect block
        let new_inode_id = fs.alloc_inode();
        // initialize inode
        let (new_inode_block_id, new_inode_block_offset) 
            = fs.get_disk_inode_pos(new_inode_id);
        get_block_cache(
            new_inode_block_id as usize,
            Arc::clone(&self.block_device)
        ).lock().modify(new_inode_block_offset, |new_inode: &mut DiskInode| {
            new_inode.initialize(DiskInodeType::File);
        });
        // 将待创建文件的目录项插入到根目录的内容中使得之后可以索引过来
        self.modify_disk_inode(|root_inode| {
            // append file in the dirent
            let file_count = (root_inode.size as usize) / DIRENT_SZ;
            let new_size = (file_count + 1) * DIRENT_SZ;
            // increase size
            self.increase_size(new_size as u32, root_inode, &mut fs);
            // write dirent
            let dirent = DirEntry::new(name, new_inode_id);
            root_inode.write_at(
                file_count * DIRENT_SZ,
                dirent.into_bytes(),
                &self.block_device,
            );
        });
        // release efs lock manually because we will acquire it again in Inode::new
        drop(fs);
        // return inode
        Some(Arc::new(Self::new(
            new_inode_id,
            self.fs.clone(),
            self.block_device.clone(),
        )))
    }

    // 收集根目录下的所有文件的文件名并以向量的形式返回回来
    // 只有根目录的 Inode 才会调用
    pub fn ls(&self) -> Vec<String> {
        let _ = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            let file_count = (disk_inode.size as usize) / DIRENT_SZ;
            let mut v: Vec<String> = Vec::new();
            for i in 0..file_count {
                let mut dirent_bytes: DirentBytes = Default::default();
                assert_eq!(
                    disk_inode.read_at(
                        i * DIRENT_SZ,
                        &mut dirent_bytes,
                        &self.block_device,
                    ),
                    DIRENT_SZ,
                );
                v.push(String::from(DirEntry::from_bytes(&dirent_bytes).name()));
            }
            v
        })
    }

    // 从根目录索引到一个文件之后可以对它进行读写
    // 这里的读写作用在字节序列的一段区间上
    pub fn read_at(&self, offset: usize, buf: &mut [u8]) -> usize {
        let _ = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            disk_inode.read_at(offset, buf, &self.block_device)
        })
    }

    // 注意在 DiskInode::write_at 之前先调用 increase_size 对自身进行扩容
    pub fn write_at(&self, offset: usize, buf: &[u8]) -> usize {
        let mut fs = self.fs.lock();
        self.modify_disk_inode(|disk_inode| {
            self.increase_size((offset + buf.len()) as u32, disk_inode, &mut fs);
            disk_inode.write_at(offset, buf, &self.block_device)
        })
    }

    // 文件清空。在索引到文件的 Inode 之后可以调用 clear 方法
    pub fn clear(&self) {
        let mut fs = self.fs.lock();
        self.modify_disk_inode(|disk_inode| {
            let size = disk_inode.size;
            let data_blocks_dealloc = disk_inode.clear_size(&self.block_device);
            assert!(data_blocks_dealloc.len() == DiskInode::total_blocks(size) as usize);
            for data_block in data_blocks_dealloc.into_iter() {
                fs.dealloc_data(data_block);
            }
        });
    }
}
