pub mod message;
pub mod range;

pub type VecBuffer = range::RangeBuffer<alloc::vec::Vec<u8>>;

impl VecBuffer {
    /// Create a buffer with the specified number of slots
    pub fn with_slot_count(slot_count: usize, slot_size: usize) -> Self {
        let len = slot_size * slot_count;
        let vec = alloc::vec![0; len];
        Self::new(vec, slot_size)
    }
}
