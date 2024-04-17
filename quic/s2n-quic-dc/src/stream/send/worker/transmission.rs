use crate::allocator::Segment;
use s2n_quic_core::{packet::number::PacketNumber, varint::VarInt};
use tracing::trace;

#[derive(Debug)]
pub struct Info<Retransmission: Segment> {
    pub packet_len: u16,
    pub retransmission: Option<Retransmission>,
    pub stream_offset: VarInt,
    pub payload_len: u16,
    pub included_fin: bool,
}

impl<S: Segment> Info<S> {
    #[inline]
    pub fn cca_len(&self) -> u16 {
        if self.payload_len == 0 {
            self.packet_len
        } else {
            self.payload_len
        }
    }

    #[inline]
    pub fn range(&self) -> core::ops::Range<VarInt> {
        self.stream_offset..self.end_offset()
    }

    #[inline]
    pub fn end_offset(&self) -> VarInt {
        self.stream_offset + VarInt::from_u16(self.payload_len)
    }

    #[inline(always)]
    pub fn on_ack(&self, packet_number: PacketNumber) {
        trace!(event = "ack", ?packet_number, range = ?self.range());
    }

    #[inline(always)]
    pub fn on_loss(&self, packet_number: PacketNumber) {
        trace!(event = "lost", ?packet_number, range = ?self.range());
    }
}
