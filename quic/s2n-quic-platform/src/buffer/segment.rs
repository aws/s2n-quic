use crate::buffer::Buffer;
use core::ops::{Deref, DerefMut, Index, IndexMut, Range};

#[derive(Debug)]
pub struct SegmentBuffer<Region> {
    region: Region,
    mtu: usize,
}

impl<Region> SegmentBuffer<Region> {
    pub fn new(region: Region, mtu: usize) -> Self {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#14.1
        //# A client MUST expand the payload of all UDP datagrams carrying
        //# Initial packets to at least the smallest allowed maximum packet size
        //# (1200 bytes)
        assert!(
            mtu >= 1200,
            "MTU must be at least 1200 for spec compatibility"
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
