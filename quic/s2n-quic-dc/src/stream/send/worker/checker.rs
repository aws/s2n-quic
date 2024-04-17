#![cfg_attr(not(debug_assertions), allow(dead_code, unused_imports))]

use s2n_quic_core::{buffer::Reader, interval_set::IntervalSet, varint::VarInt};

#[cfg(debug_assertions)]
macro_rules! run {
    ($($tt:tt)*) => {
        $($tt)*
    }
}

#[cfg(not(debug_assertions))]
macro_rules! run {
    ($($tt:tt)*) => {};
}

#[cfg(debug_assertions)]
#[derive(Clone, Debug, Default)]
pub struct Checker {
    acked_ranges: IntervalSet<VarInt>,
    largest_transmitted_offset: VarInt,
    max_data: VarInt,
    highest_seen_offset: Option<VarInt>,
    final_offset: Option<VarInt>,
}

#[cfg(not(debug_assertions))]
#[derive(Clone, Debug, Default)]
pub struct Checker {}

#[allow(unused_variables)]
impl Checker {
    #[inline(always)]
    pub fn check_payload(&mut self, payload: &impl Reader) {
        run!({
            if let Some(final_offset) = payload.final_offset() {
                self.on_final_offset(final_offset);
            }
            self.on_stream_offset(
                payload.current_offset(),
                payload.buffered_len().min(u16::MAX as _) as _,
            );
        });
    }

    #[inline(always)]
    pub fn on_ack(&mut self, offset: VarInt, payload_len: u16) {
        run!(if payload_len > 0 {
            self.acked_ranges
                .insert(offset..offset + VarInt::from_u16(payload_len))
                .unwrap();
        });
    }

    #[inline(always)]
    pub fn on_max_data(&mut self, max_data: VarInt) {
        run!({
            self.max_data = self.max_data.max(max_data);
        });
    }

    #[inline(always)]
    pub fn check_pending_packets<S: super::Segment, R: super::Segment>(
        &self,
        packets: &super::PacketMap<R>,
        retransmissions: &super::BinaryHeap<super::Retransmission<S>>,
    ) {
        run!({
            let largest_transmitted_offset = self.largest_transmitted_offset;
            if largest_transmitted_offset == 0u64 {
                return;
            }

            let mut missing = IntervalSet::new();
            missing
                .insert(VarInt::ZERO..largest_transmitted_offset)
                .unwrap();
            // remove all of the ranges we've acked
            missing.difference(&self.acked_ranges).unwrap();

            for (_pn, packet) in packets.iter() {
                let offset = packet.data.stream_offset;
                let payload_len = packet.data.payload_len;
                if payload_len > 0 {
                    missing
                        .remove(offset..offset + VarInt::from_u16(payload_len))
                        .unwrap();
                }
            }

            for packet in retransmissions.iter() {
                let offset = packet.stream_offset;
                let payload_len = packet.payload_len;
                if payload_len > 0 {
                    missing
                        .remove(offset..offset + VarInt::from_u16(payload_len))
                        .unwrap();
                }
            }

            assert!(
                missing.is_empty(),
                "missing ranges for retransmission {missing:?}"
            );
        });
    }

    #[inline(always)]
    pub fn on_stream_transmission(
        &mut self,
        offset: VarInt,
        payload_len: u16,
        is_retransmission: bool,
        is_probe: bool,
    ) {
        run!({
            self.on_stream_offset(offset, payload_len);

            if !is_retransmission && !is_probe {
                assert_eq!(self.largest_transmitted_offset, offset);
            }

            let end_offset = offset + VarInt::from_u16(payload_len);
            self.largest_transmitted_offset = self.largest_transmitted_offset.max(end_offset);

            assert!(self.largest_transmitted_offset <= self.max_data);
        });
    }

    #[inline(always)]
    pub fn on_stream_offset(&mut self, offset: VarInt, payload_len: u16) {
        run!({
            if let Some(final_offset) = self.final_offset {
                assert!(offset <= final_offset);
            }

            match self.highest_seen_offset.as_mut() {
                Some(prev) => *prev = (*prev).max(offset),
                None => self.highest_seen_offset = Some(offset),
            }
        });
    }

    #[inline(always)]
    fn on_final_offset(&mut self, final_offset: VarInt) {
        run!({
            self.on_stream_offset(final_offset, 0);

            match self.final_offset {
                Some(prev) => assert_eq!(prev, final_offset),
                None => self.final_offset = Some(final_offset),
            }
        });
    }
}
