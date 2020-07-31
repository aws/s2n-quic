use crate::buffer::{Buffer, SegmentBuffer};
use core::ops::{Index, IndexMut};

// TODO decide on better defaults
const DEFAULT_MESSAGE_COUNT: usize = 4096;
const DEFAULT_MTU: usize = 1500;

pub struct VecBuffer(SegmentBuffer<alloc::vec::Vec<u8>>);

impl VecBuffer {
    /// Create a contiguous buffer with the specified number of messages
    pub fn new(message_count: usize, mtu: usize) -> Self {
        let len = message_count * mtu;
        let vec = alloc::vec![0; len];
        Self(SegmentBuffer::new(vec, mtu))
    }
}

impl Default for VecBuffer {
    fn default() -> Self {
        // when testing this crate, make buffers smaller to avoid
        // repeated large allocations
        if cfg!(test) {
            Self::new(64, 1200)
        } else {
            Self::new(DEFAULT_MESSAGE_COUNT, DEFAULT_MTU)
        }
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
