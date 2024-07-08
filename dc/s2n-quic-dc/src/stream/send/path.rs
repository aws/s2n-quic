// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::{
    fmt,
    sync::atomic::{AtomicU64, Ordering},
};
use s2n_quic_core::{inet::ExplicitCongestionNotification, varint::VarInt};

/// Contains the current state of a transmission path
pub struct State {
    info: AtomicU64,
    next_expected_control_packet: AtomicU64,
}

impl fmt::Debug for State {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.load().fmt(f)
    }
}

impl State {
    #[inline]
    pub fn new(info: Info) -> Self {
        Self {
            info: AtomicU64::new(Self::encode_info(
                info.ecn,
                info.send_quantum,
                info.max_datagram_size,
            )),
            next_expected_control_packet: AtomicU64::new(
                info.next_expected_control_packet.as_u64(),
            ),
        }
    }

    /// Loads a relaxed view of the current path state
    #[inline]
    pub fn load(&self) -> Info {
        // use relaxed since it's ok to be slightly out of sync with the current MTU/send_quantum
        let data = self.info.load(Ordering::Relaxed);
        let (ecn, send_quantum, max_datagram_size) = Self::decode_info(data);

        let next_expected_control_packet =
            self.next_expected_control_packet.load(Ordering::Relaxed);
        let next_expected_control_packet =
            VarInt::new(next_expected_control_packet).unwrap_or(VarInt::MAX);

        Info {
            max_datagram_size,
            send_quantum,
            ecn,
            next_expected_control_packet,
        }
    }

    #[inline]
    pub fn update_info(
        &self,
        ecn: ExplicitCongestionNotification,
        send_quantum: u8,
        max_datagram_size: u16,
    ) {
        let info = Self::encode_info(ecn, send_quantum, max_datagram_size);
        self.info.store(info, Ordering::Relaxed);
    }

    #[inline]
    fn decode_info(mut data: u64) -> (ExplicitCongestionNotification, u8, u16) {
        let max_datagram_size = data as u16;
        data >>= 16;

        let send_quantum = data as u8;
        data >>= 8;

        let ecn = data as u8;
        let ecn = ExplicitCongestionNotification::new(ecn);
        data >>= 8;

        // TODO can we store pacing rate in the remaining bits?

        debug_assert_eq!(data, 0, "unexpected extra data");

        (ecn, send_quantum, max_datagram_size)
    }

    #[inline]
    fn encode_info(
        ecn: ExplicitCongestionNotification,
        send_quantum: u8,
        max_datagram_size: u16,
    ) -> u64 {
        let mut data = 0u64;

        data |= ecn as u8 as u64;
        data <<= 8;

        data |= send_quantum as u64;
        data <<= 16;

        data |= max_datagram_size as u64;

        data
    }

    #[inline]
    pub fn set_next_expected_control_packet(&self, next_expected_control_packet: VarInt) {
        self.next_expected_control_packet
            .store(next_expected_control_packet.as_u64(), Ordering::Relaxed);
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Info {
    pub max_datagram_size: u16,
    pub send_quantum: u8,
    pub ecn: ExplicitCongestionNotification,
    pub next_expected_control_packet: VarInt,
}

impl Info {
    /// Returns the maximum number of flow credits for the current path info
    #[inline]
    pub fn max_flow_credits(&self, max_header_len: usize, max_segments: usize) -> u64 {
        // trim off the headers since those don't count for flow control
        let max_payload_size_per_segment = self.max_datagram_size as usize - max_header_len;
        // clamp the number of segments we can transmit in a single burst
        let max_segments = max_segments.min(self.send_quantum as usize);

        let max_payload_size = max_payload_size_per_segment * max_segments;

        max_payload_size as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bolero::check;

    /// Ensures encode/decode functions correctly round-trip
    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn codec_inverse_pair() {
        check!()
            .with_type()
            .cloned()
            .for_each(|(ecn, send_quantum, max_datagram_size)| {
                let actual =
                    State::decode_info(State::encode_info(ecn, send_quantum, max_datagram_size));

                assert_eq!((ecn, send_quantum, max_datagram_size), actual);
            })
    }
}
