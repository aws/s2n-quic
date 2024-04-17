use core::sync::atomic::{AtomicU64, Ordering};
use s2n_quic_core::{inet::ExplicitCongestionNotification, varint::VarInt};

/// Contains the current state of a transmission path
pub struct State {
    info: AtomicU64,
    next_expected_control_packet: AtomicU64,
}

impl State {
    /// Loads a relaxed view of the current path state
    #[inline]
    pub fn load(&self) -> Info {
        // use relaxed since it's ok to be slightly out of sync with the current MTU/send_quantum
        let mut data = self.info.load(Ordering::Relaxed);

        let mtu = data as u16;
        data >>= 16;

        let send_quantum = data as u8;
        data >>= 8;

        let ecn = data as u8;
        let ecn = ExplicitCongestionNotification::new(ecn);
        data >>= 8;

        // TODO can we store pacing rate in the remaining bits?

        debug_assert_eq!(data, 0, "unexpected extra data");

        let next_expected_control_packet =
            self.next_expected_control_packet.load(Ordering::Relaxed);
        let next_expected_control_packet =
            VarInt::new(next_expected_control_packet).unwrap_or(VarInt::MAX);

        Info {
            mtu,
            send_quantum,
            ecn,
            next_expected_control_packet,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Info {
    pub mtu: u16,
    pub send_quantum: u8,
    pub ecn: ExplicitCongestionNotification,
    pub next_expected_control_packet: VarInt,
}

impl Info {
    /// Returns the maximum number of flow credits for the current path info
    #[inline]
    pub fn max_flow_credits(&self, max_header_len: usize, max_segments: usize) -> u64 {
        // trim off the headers since those don't count for flow control
        let max_payload_size_per_segment = self.mtu as usize - max_header_len;
        // clamp the number of segments we can transmit in a single burst
        let max_segments = max_segments.min(self.send_quantum as usize);

        let max_payload_size = max_payload_size_per_segment * max_segments;

        max_payload_size as u64
    }
}
