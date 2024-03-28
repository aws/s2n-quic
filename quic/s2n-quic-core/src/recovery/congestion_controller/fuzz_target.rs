// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event,
    packet::number::PacketNumberSpace,
    path,
    path::MINIMUM_MAX_DATAGRAM_SIZE,
    random,
    recovery::{
        bbr::BbrCongestionController, congestion_controller::PathPublisher, CongestionController,
        CubicCongestionController, RttEstimator,
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
        #[generator(0..=9000)]
        bytes_sent: u16,
        app_limited: Option<bool>,
    },
    AckReceived {
        index: u8,
        #[generator(1..=255)]
        count: u8,
        #[generator(1..=2000)]
        rtt: u16,
    },
    PacketLost {
        index: u8,
    },
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

    fn apply(&mut self, operation: &Operation, rng: &mut dyn random::Generator) {
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
            Operation::AckReceived { index, count, rtt } => {
                self.on_ack_received(*index, *count, Duration::from_millis(*rtt as u64), rng);
            }
            Operation::PacketLost { index } => {
                self.on_packet_lost(*index, rng);
            }
            Operation::ExplicitCongestion { ce_count } => self.on_explicit_congestion(*ce_count),
            Operation::MtuUpdated { mtu } => self.on_mtu_updated(*mtu),
            Operation::PacketDiscarded => self.on_packet_discarded(),
        }
    }

    fn on_packet_sent(&mut self, count: u8, bytes_sent: u16, app_limited: Option<bool>) {
        let mut publisher = event::testing::Publisher::no_snapshot();
        let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());

        for _ in 0..count {
            if !self.subject.is_congestion_limited()
                || self.subject.requires_fast_retransmission()
                || bytes_sent == 0
            {
                let packet_info = self.subject.on_packet_sent(
                    self.timestamp,
                    bytes_sent as usize,
                    app_limited,
                    &self.rtt_estimator,
                    &mut publisher,
                );
                self.sent_packets.push_back(SentPacketInfo {
                    sent_bytes: bytes_sent,
                    time_sent: self.timestamp,
                    cc_packet_info: packet_info,
                });
            }
        }
    }

    fn on_ack_received(
        &mut self,
        index: u8,
        count: u8,
        rtt: Duration,
        rng: &mut dyn random::Generator,
    ) {
        let mut publisher = event::testing::Publisher::no_snapshot();
        let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());
        let index = (index as usize).min(self.sent_packets.len().saturating_sub(1));
        let mut rtt_updated = false;

        // Acknowledge `count` amount of packets, starting at the random `index`
        for _ in 0..count {
            if let Some(sent_packet_info) = self.sent_packets.remove(index) {
                if sent_packet_info.sent_bytes > 0 {
                    // Update the RTT once for each ack range received
                    if !rtt_updated {
                        self.on_rtt_updated(sent_packet_info.time_sent, rtt);
                        rtt_updated = true;
                    }

                    // `recovery::Manager` does not call `on_ack` if sent_bytes = 0
                    self.subject.on_ack(
                        sent_packet_info.time_sent,
                        sent_packet_info.sent_bytes as usize,
                        sent_packet_info.cc_packet_info,
                        &self.rtt_estimator,
                        rng,
                        self.timestamp,
                        &mut publisher,
                    );
                }
            } else {
                break;
            }
        }
    }

    fn on_packet_lost(&mut self, index: u8, rng: &mut dyn random::Generator) {
        let mut publisher = event::testing::Publisher::no_snapshot();
        let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());
        let index = (index as usize).min(self.sent_packets.len().saturating_sub(1));

        // Report the packet at the random `index` as lost
        if let Some(sent_packet_info) = self.sent_packets.remove(index) {
            if sent_packet_info.sent_bytes > 0 {
                // `recovery::Manager` does not call `on_packet_lost` if sent_bytes = 0
                self.subject.on_packet_lost(
                    sent_packet_info.sent_bytes as u32,
                    sent_packet_info.cc_packet_info,
                    false,
                    false,
                    rng,
                    self.timestamp,
                    &mut publisher,
                );
            }
        }
    }

    fn on_rtt_updated(&mut self, time_sent: Timestamp, rtt: Duration) {
        let mut publisher = event::testing::Publisher::no_snapshot();
        let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());

        self.rtt_estimator.update_rtt(
            Duration::ZERO,
            rtt,
            self.timestamp,
            false,
            PacketNumberSpace::Initial,
        );
        self.subject.on_rtt_update(
            time_sent,
            self.timestamp,
            &self.rtt_estimator,
            &mut publisher,
        );
    }

    fn on_explicit_congestion(&mut self, ce_count: u64) {
        let mut publisher = event::testing::Publisher::no_snapshot();
        let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());
        let ce_count = ce_count.min(self.sent_packets.len() as u64);
        self.subject
            .on_explicit_congestion(ce_count, self.timestamp, &mut publisher)
    }

    fn on_packet_discarded(&mut self) {
        let mut publisher = event::testing::Publisher::no_snapshot();
        let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());
        if let Some(sent_packet_info) = self.sent_packets.pop_front() {
            self.subject
                .on_packet_discarded(sent_packet_info.sent_bytes as usize, &mut publisher)
        }
    }

    fn on_mtu_updated(&mut self, mtu: u16) {
        let mut publisher = event::testing::Publisher::no_snapshot();
        let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());
        self.subject.on_mtu_update(mtu, &mut publisher)
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

#[cfg_attr(miri, ignore)]
#[test]
fn cubic_fuzz() {
    check!()
        .with_generator((
            MINIMUM_MAX_DATAGRAM_SIZE..=9000,
            gen(),
            gen::<Vec<Operation>>(),
        ))
        .for_each(|(max_datagram_size, seed, operations)| {
            let mut model = Model::new(CubicCongestionController::new(*max_datagram_size));
            let mut rng = random::testing::Generator(*seed);

            for operation in operations.iter() {
                model.apply(operation, &mut rng);
            }

            model.invariants();
        });
}

#[cfg_attr(miri, ignore)]
#[test]
fn bbr_fuzz() {
    check!()
        .with_generator((
            MINIMUM_MAX_DATAGRAM_SIZE..=9000,
            gen(),
            gen::<Vec<Operation>>(),
        ))
        .for_each(|(max_datagram_size, seed, operations)| {
            let mut model = Model::new(BbrCongestionController::new(*max_datagram_size));
            let mut rng = random::testing::Generator(*seed);

            for operation in operations.iter() {
                model.apply(operation, &mut rng);
            }

            model.invariants();
        });
}
