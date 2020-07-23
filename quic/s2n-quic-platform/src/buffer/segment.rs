use crate::buffer::Buffer;
use core::ops::{Deref, DerefMut, Index, IndexMut, Range};

#[derive(Debug)]
pub struct SegmentBuffer<Region> {
    region: Region,
    mtu: usize,
}

impl<Region> SegmentBuffer<Region> {
    pub fn new(region: Region, mtu: usize) -> Self {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#14
        //# The payload of a UDP datagram carrying the first Initial packet MUST
        //# be expanded to at least 1200 bytes, by adding PADDING frames to the
        //# Initial packet and/or by coalescing the Initial packet (see
        //# Section 12.2).  Sending a UDP datagram of this size ensures that the
        //# network path supports a reasonable Maximum Transmission Unit (MTU),
        //# and helps reduce the amplitude of amplification attacks caused by
        //# server responses toward an unverified client address; see Section 8.
        assert!(
            mtu >= 1200,
            "slots must be at least 1200 for spec compatibility"
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
