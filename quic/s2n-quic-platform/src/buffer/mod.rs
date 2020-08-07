mod segment;
mod vec;
// TODO support mmap buffers

pub use segment::*;
pub use vec::*;

use core::ops::{Index, IndexMut};

pub trait Buffer: Index<usize, Output = [u8]> + IndexMut<usize> {
    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn mtu(&self) -> usize;
}

pub mod default {
    pub use super::vec::VecBuffer as Buffer;
}
