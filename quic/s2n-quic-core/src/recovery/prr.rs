// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::counter::Counter;

/// environment variable for using PRR
#[cfg(feature = "std")]
const S2N_ENABLE_PRR: &str = "S2N_ENABLE_PRR";

/// Proportional Rate Reduction
/// https://www.rfc-editor.org/rfc/rfc6937.html
#[derive(Clone, Debug)]
pub struct Prr {
    /// Total bytes sent during recovery (prr_out)
    bytes_sent: usize,

    /// Total bytes delivered during recovery (prr_delivered)
    bytes_delivered: usize,

    /// FlightSize at the start of recovery (aka recoverfs)
    bytes_in_flight_at_recovery: usize,

    /// a local variable "sndcnt", which indicates exactly how
    /// many bytes should be sent in response to each ACK.
    bytes_allowed: usize,
}

impl Prr {
    pub fn new() -> Self {
        Self {
            bytes_in_flight_at_recovery: 0,
            bytes_sent: 0,
            bytes_delivered: 0,
            bytes_allowed: 0,
        }
    }

    pub fn on_congestion_event(&mut self, bytes_in_flight: Counter<u32>) {
        // on congestion window, reset all counters except for bytes_in_flight
        self.bytes_in_flight_at_recovery = *bytes_in_flight as usize;
        self.bytes_sent = 0;
        self.bytes_delivered = 0;
        self.bytes_allowed = 0;
    }

    pub fn on_packet_sent(&mut self, bytes_sent: usize) {
        self.bytes_sent += bytes_sent;

        self.bytes_allowed = self.bytes_allowed.saturating_sub(bytes_sent);
    }

    pub fn on_ack(
        &mut self,
        bytes_acknowledged: usize,
        bytes_in_flight: Counter<u32>,
        slow_start_threshold: usize,
        max_datagram_size: u16,
    ) {
        let bytes_in_flight = *bytes_in_flight as usize;
        self.bytes_delivered += bytes_acknowledged;

        let prr_delivered = self.bytes_delivered;
        let ssthresh = slow_start_threshold;
        let recover_fs = self.bytes_in_flight_at_recovery;
        let prr_out = self.bytes_sent;

        let sndcnt = if bytes_in_flight > slow_start_threshold {
            if self.bytes_in_flight_at_recovery == 0 {
                0
            } else {
                //= https://www.rfc-editor.org/rfc/rfc6937.html#section-3.1
                //# Proportional Rate Reduction
                //# sndcnt = CEIL(prr_delivered * ssthresh / RecoverFS) - prr_out

                ((prr_delivered * ssthresh
                        // get around floating point conversions
                        + recover_fs
                    - 1)
                    / recover_fs)
                    .saturating_sub(prr_out)
            }
        } else {
            // Slow Start Reduction Bound
            //= https://www.rfc-editor.org/rfc/rfc6937.html#section-3.1
            //# // PRR-SSRB
            //# limit = MAX(prr_delivered - prr_out, DeliveredData) + MSS
            let limit = prr_delivered
                .saturating_sub(prr_out)
                .max(bytes_acknowledged)
                + max_datagram_size as usize;

            //# Attempt to catch up, as permitted by limit
            slow_start_threshold
                .saturating_sub(bytes_in_flight)
                .min(limit)
        };

        self.bytes_allowed = if prr_out == 0 && sndcnt == 0 {
            // updated safeguard from https://www.ietf.org/archive/id/draft-ietf-tcpm-prr-rfc6937bis-02.html#name-changes-from-rfc-6937
            // Force a fast retransmit upon entering recovery
            sndcnt + max_datagram_size as usize
        } else {
            sndcnt
        };
    }

    pub fn can_transmit(&self, datagram_size: u16) -> bool {
        self.bytes_allowed >= datagram_size as usize
    }

    #[cfg(feature = "std")]
    pub fn is_enabled(&self) -> bool {
        use once_cell::sync::OnceCell;
        static USE_PRR: OnceCell<bool> = OnceCell::new();
        *USE_PRR.get_or_init(|| std::env::var(S2N_ENABLE_PRR).is_ok())
    }

    #[cfg(not(feature = "std"))]
    pub fn is_enabled(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod test {
    use crate::{counter::Counter, recovery::prr::Prr};

    #[test]
    fn prr_example() {
        // PRR (pipe > ssthresh)
        let datagram_size: u16 = 1;

        let mut pipe = Counter::new((10 * datagram_size).into());
        let recover_fs = Counter::new((10 * datagram_size).into());
        let ssthresh = (7 * datagram_size).into();

        let mut prr = Prr::new();
        assert!(!prr.can_transmit(datagram_size));

        // enter recovery
        prr.on_congestion_event(recover_fs);
        assert!(!prr.can_transmit(datagram_size));

        let transmission_allowed = vec![true, true, true, false, true, true, false];

        let mut transmission = false;
        let mut total_transmits = 0;

        // run thru iterations
        for i in 0..transmission_allowed.len() {
            if transmission {
                prr.on_packet_sent(datagram_size.into());
            }

            pipe -= datagram_size;
            prr.on_ack(datagram_size.into(), pipe, ssthresh, datagram_size.into());

            transmission = *transmission_allowed.get(i).unwrap();
            let bytes = if transmission {
                datagram_size as usize
            } else {
                0
            };

            if transmission {
                total_transmits += 1;
            }

            assert_eq!(prr.can_transmit(datagram_size), transmission);
            assert_eq!(prr.bytes_allowed, bytes);
            pipe += bytes as u32;
        }

        // 5 transmission within 7 acks
        assert_eq!(total_transmits, 5);
    }

    #[test]
    fn ssrb_example() {
        // pipe ≤ ssthresh, with SSRB

        let datagram_size: u16 = 1;
        let mut pipe = Counter::new((4 * datagram_size).into());
        let recover_fs = Counter::new((10 * datagram_size).into());
        let ssthresh = (5 * datagram_size).into();

        let mut prr = Prr::new();
        assert!(!prr.can_transmit(datagram_size));

        // enter recovery
        prr.on_congestion_event(recover_fs);
        assert!(!prr.can_transmit(datagram_size));

        let mut total_transmits = 0;

        // receives 1 ack
        pipe -= datagram_size;
        prr.on_ack(datagram_size.into(), pipe, ssthresh, datagram_size.into());

        // make 2 transmissions
        assert!(prr.can_transmit(datagram_size));
        assert_eq!(prr.bytes_allowed, 2);
        pipe += prr.bytes_allowed as u16;
        total_transmits += prr.bytes_allowed;
        prr.on_packet_sent(datagram_size.into());

        // allow 1 transmission per ack
        for _ in 0..8 {
            pipe -= datagram_size;
            prr.on_ack(datagram_size.into(), pipe, ssthresh, datagram_size.into());
            assert!(prr.can_transmit(datagram_size));
            assert_eq!(prr.bytes_allowed, 1);
            pipe += prr.bytes_allowed as u16;
            total_transmits += prr.bytes_allowed;
            prr.on_packet_sent(prr.bytes_allowed);
        }

        assert_eq!(total_transmits, 10);
    }
}
