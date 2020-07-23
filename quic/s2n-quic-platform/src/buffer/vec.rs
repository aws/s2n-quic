use crate::buffer::{Buffer, SegmentBuffer};
use core::ops::{Index, IndexMut};

pub struct VecBuffer(SegmentBuffer<alloc::vec::Vec<u8>>);

impl VecBuffer {
    /// Create a contiguous buffer with the specified number of messages
    pub fn new(message_len: usize, mtu: usize) -> Self {
        let len = message_len * mtu;
        let vec = alloc::vec![0; len];
        Self(SegmentBuffer::new(vec, mtu))
    }
}

impl Buffer for VecBuffer {
    fn len(&self) -> usize {
        self.0.len()
    }

    fn mtu(&self) -> usize {
        self.0.mtu()
    }
}

impl Index<usize> for VecBuffer {
    type Output = [u8];

    fn index(&self, index: usize) -> &Self::Output {
        self.0.index(index)
    }
}

impl IndexMut<usize> for VecBuffer {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        self.0.index_mut(index)
    }
}
