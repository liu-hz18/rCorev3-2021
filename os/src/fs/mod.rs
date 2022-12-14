mod pipe;
mod stdio;
mod mail_box;
mod inode;

use crate::mm::UserBuffer;
pub trait File : Send + Sync {
    fn readable(&self) -> bool;
    fn writable(&self) -> bool;
    fn read(&self, buf: UserBuffer) -> usize;
    fn write(&self, buf: UserBuffer) -> usize;
    fn inode_id(&self) -> usize;
    fn nlink(&self) -> usize;
}

pub use pipe::{Pipe, make_pipe};
pub use stdio::{Stdin, Stdout};
pub use mail_box::MailBox;
pub use inode::{OSInode, open_file, OpenFlags, list_apps};
pub use inode::{link, unlink, map};
