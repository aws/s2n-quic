// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    packet::number::PacketNumberSpace,
    path::MINIMUM_MTU,
    random,
    recovery::{
        bbr::BbrCongestionController, CongestionController, CubicCongestionController, RttEstimator,
    },
    time::{testing::Clock, Clock as _, Timestamp},
};
use bolero::{check, generator::*};
use core::time::Duration;
use std::collections::VecDeque;

struct SentPacketInfo<PacketInfo> {
    sent_bytes: u16,
    time_sent: Timestamp,
    cc_packet_info: PacketInfo,
}

#[derive(Debug, TypeGenerator)]
enum Operation {
    IncrementTime {
        /// The milli-second value by which to increase the timestamp
        millis: u16,
    },
    PacketSent {
        #[generator(1..=255)]
        count: u8,
        #[generator(1200..=9000)]
        bytes_sent: u16,
        app_limited: Option<bool>,
    },
    RttUpdated {
        #[generator(1..=2000)]
        millis: u64,
    },
    AckReceived {
        #[generator(1..=255)]
        count: u8,
    },
    PacketLost,
    ExplicitCongestion {
        #[generator(1..=255)]
        ce_count: u64,
    },
    MtuUpdated {
        #[generator(1200..=9000)]
        mtu: u16,
    },
    PacketDiscarded,
}

struct Model<CC: CongestionController> {
    /// The congestion controller being fuzzed
    subject: CC,
    /// Tracks packets inflight
    sent_packets: VecDeque<SentPacketInfo<CC::PacketInfo>>,
    /// The round trip time estimator
    rtt_estimator: RttEstimator,
    /// A monotonically increasing timestamp
    timestamp: Timestamp,
}

impl<CC: CongestionController> Model<CC> {
    fn new(congestion_controller: CC) -> Self {
        Self {
            subject: congestion_controller,
            sent_packets: VecDeque::new(),
            rtt_estimator: RttEstimator::default(),
            timestamp: Clock::default().get_time(),
        }
    }

    fn apply<Rnd: random::Generator>(&mut self, operation: &Operation, rng: &mut Rnd) {
        match operation {
            Operation::IncrementTime { millis } => {
                self.timestamp += Duration::from_millis(*millis as u64);
            }
            Operation::PacketSent {
                count,
                bytes_sent,
                app_limited,
            } => {
                self.on_packet_sent(*count, *bytes_sent, *app_limited);
            }
            Operation::RttUpdated { millis } => self.on_rtt_updated(Duration::from_millis(*millis)),
            Operation::AckReceived { count } => {
                self.on_ack_received(*count, rng);
            }
            Operation::PacketLost => {
                self.on_packet_lost(rng);
            }
            Operation::ExplicitCongestion { ce_count } => self.on_explicit_congestion(*ce_count),
            Operation::MtuUpdated { mtu } => self.on_mtu_updated(*mtu),
            Operation::PacketDiscarded => self.on_packet_discarded(),
        }
    }

    fn on_packet_sent(&mut self, count: u8, bytes_sent: u16, app_limited: Option<bool>) {
        for _ in 0..count {
            let packet_info = self.subject.on_packet_sent(
                self.timestamp,
                bytes_sent as usize,
                app_limited,
                &self.rtt_estimator,
            );
            self.sent_packets.push_back(SentPacketInfo {
                sent_bytes: bytes_sent,
                time_sent: self.timestamp,
                cc_packet_info: packet_info,
            });
        }
    }

    fn on_ack_received<Rnd: random::Generator>(&mut self, count: u8, rng: &mut Rnd) {
        for _ in 0..count {
            if let Some(sent_packet_info) = self.sent_packets.pop_front() {
                self.subject.on_ack(
                    sent_packet_info.time_sent,
                    sent_packet_info.sent_bytes as usize,
                    sent_packet_info.cc_packet_info,
                    &self.rtt_estimator,
                    rng,
                    self.timestamp,
                );
            } else {
                break;
            }
        }
    }

    fn on_packet_lost<Rnd: random::Generator>(&mut self, rng: &mut Rnd) {
        if let Some(sent_packet_info) = self.sent_packets.pop_front() {
            self.subject.on_packet_lost(
                sent_packet_info.sent_bytes as u32,
                sent_packet_info.cc_packet_info,
                false,
                false,
                rng,
                self.timestamp,
            );
        }
    }

    fn on_rtt_updated(&mut self, rtt: Duration) {
        if let Some(sent_packet_info) = self.sent_packets.front() {
            self.rtt_estimator.update_rtt(
                Duration::ZERO,
                rtt,
                self.timestamp,
                false,
                PacketNumberSpace::Initial,
            );
            self.subject.on_rtt_update(
                sent_packet_info.time_sent,
                self.timestamp,
                &self.rtt_estimator,
            );
        }
    }

    fn on_explicit_congestion(&mut self, ce_count: u64) {
        let ce_count = ce_count.min(self.sent_packets.len() as u64);
        self.subject
            .on_explicit_congestion(ce_count, self.timestamp)
    }

    fn on_packet_discarded(&mut self) {
        if let Some(sent_packet_info) = self.sent_packets.pop_front() {
            self.subject
                .on_packet_discarded(sent_packet_info.sent_bytes as usize)
        }
    }

    fn on_mtu_updated(&mut self, mtu: u16) {
        self.subject.on_mtu_update(mtu)
    }

    fn invariants(&self) {
        let bytes_in_flight: u32 = self
            .sent_packets
            .iter()
            .map(|sent_packet_info| sent_packet_info.sent_bytes as u32)
            .sum();

        assert_eq!(bytes_in_flight, self.subject.bytes_in_flight());
    }
}

#[test]
fn cubic_fuzz() {
    check!()
        .with_generator((MINIMUM_MTU..=9000, 0..255, gen::<Vec<Operation>>()))
        .for_each(|(max_datagram_size, seed, operations)| {
            let mut model = Model::new(CubicCongestionController::new(*max_datagram_size));
            let mut rng = random::testing::Generator(*seed);

            for operation in operations.iter() {
                model.apply(operation, &mut rng);
            }

            model.invariants();
        });
}

#[test]
fn bbr_fuzz() {
    check!()
        .with_generator((MINIMUM_MTU..=9000, 0..255, gen::<Vec<Operation>>()))
        .for_each(|(max_datagram_size, seed, operations)| {
            let mut model = Model::new(BbrCongestionController::new(*max_datagram_size));
            let mut rng = random::testing::Generator(*seed);

            for operation in operations.iter() {
                model.apply(operation, &mut rng);
            }

            model.invariants();
        });
}
