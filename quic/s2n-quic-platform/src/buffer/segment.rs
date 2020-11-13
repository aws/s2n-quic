use crate::buffer::Buffer;
use core::ops::{Deref, DerefMut, Index, IndexMut, Range};
use s2n_quic_core::path::MINIMUM_MTU;

#[derive(Debug)]
pub struct SegmentBuffer<Region> {
    region: Region,
    mtu: usize,
}

impl<Region> SegmentBuffer<Region> {
    pub fn new(region: Region, mtu: usize) -> Self {
        assert!(
            mtu >= (MINIMUM_MTU as usize),
            "MTU must be at least {} for spec compatibility",
            MINIMUM_MTU
        );
        Self { region, mtu }
    }

    const fn byte_range(&self, index: usize) -> Range<usize> {
        let start = index * self.mtu;
        let end = start + self.mtu;
        start..end
    }
}

impl<Region: Deref<Target = [u8]> + DerefMut<Target = [u8]>> Buffer for SegmentBuffer<Region> {
    fn len(&self) -> usize {
        self.region.len() / self.mtu
    }

    fn is_empty(&self) -> bool {
        self.region.is_empty()
    }

    fn mtu(&self) -> usize {
        self.mtu
    }
}

impl<Region: Deref<Target = [u8]>> Index<usize> for SegmentBuffer<Region> {
    type Output = [u8];

    fn index(&self, index: usize) -> &Self::Output {
        let range = self.byte_range(index);
        &self.region[range]
    }
}

impl<Region: DerefMut<Target = [u8]>> IndexMut<usize> for SegmentBuffer<Region> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        let range = self.byte_range(index);
        &mut self.region[range]
    }
}
