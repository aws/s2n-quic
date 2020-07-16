use crate::io::buffer::message::MessageBuffer;
use core::ops::{Deref, DerefMut, Index, IndexMut, Range};

#[derive(Debug)]
pub struct RangeBuffer<Buffer> {
    buffer: Buffer,
    slot_size: usize,
}

impl<Buffer> RangeBuffer<Buffer> {
    pub fn new(buffer: Buffer, slot_size: usize) -> Self {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-24.txt#14
        //# The payload of a UDP datagram carrying the first Initial packet MUST
        //# be expanded to at least 1200 bytes, by adding PADDING frames to the
        //# Initial packet and/or by coalescing the Initial packet (see
        //# Section 12.2).  Sending a UDP datagram of this size ensures that the
        //# network path supports a reasonable Maximum Transmission Unit (MTU),
        //# and helps reduce the amplitude of amplification attacks caused by
        //# server responses toward an unverified client address; see Section 8.
        assert!(
            slot_size >= 1200,
            "slots must be at least 1200 for spec compatibility"
        );
        Self { buffer, slot_size }
    }

    const fn byte_range(&self, index: usize) -> Range<usize> {
        let start = index * self.slot_size;
        let end = start + self.slot_size;
        start..end
    }
}

impl<Buffer: Deref<Target = [u8]> + DerefMut<Target = [u8]>> MessageBuffer for RangeBuffer<Buffer> {
    fn len(&self) -> usize {
        self.buffer.len() / self.slot_size
    }

    fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    fn slot_size(&self) -> usize {
        self.slot_size
    }
}

impl<Buffer: Deref<Target = [u8]>> Index<usize> for RangeBuffer<Buffer> {
    type Output = [u8];

    fn index(&self, index: usize) -> &Self::Output {
        let range = self.byte_range(index);
        &self.buffer[range]
    }
}

impl<Buffer: DerefMut<Target = [u8]>> IndexMut<usize> for RangeBuffer<Buffer> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        let range = self.byte_range(index);
        &mut self.buffer[range]
    }
}
