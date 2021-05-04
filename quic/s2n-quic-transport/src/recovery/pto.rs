// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{contexts::WriteContext, timer::VirtualTimer, transmission};
use core::time::Duration;
use s2n_quic_core::{frame, time::Timestamp};

/// Manages the probe time out calculation and probe packet transmission
#[derive(Debug, Default)]
pub(crate) struct Pto {
    pub timer: VirtualTimer,
    pub state: PtoState,
    pub max_ack_delay: Duration,
}

#[derive(Debug, PartialEq)]
pub(crate) enum PtoState {
    Idle,
    RequiresTransmission(u8),
}

impl Default for PtoState {
    fn default() -> Self {
        Self::Idle
    }
}

impl Pto {
    /// Constructs a new `Pto` with the given `max_ack_delay`
    pub fn new(max_ack_delay: Duration) -> Self {
        Self {
            max_ack_delay,
            ..Self::default()
        }
    }

    /// Returns an iterator containing the probe timeout timestamp
    pub fn timers(&self) -> impl Iterator<Item = Timestamp> {
        self.timer.iter()
    }

    /// Called when a timeout has occurred. Returns true if the PTO timer had expired.
    pub fn on_timeout(&mut self, packets_in_flight: bool, timestamp: Timestamp) -> bool {
        if self.timer.poll_expiration(timestamp).is_ready() {
            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.4
            //# When a PTO timer expires, a sender MUST send at least one ack-
            //# eliciting packet in the packet number space as a probe.

            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.2.1
            //# Since the server could be blocked until more datagrams are received
            //# from the client, it is the client's responsibility to send packets to
            //# unblock the server until it is certain that the server has finished
            //# its address validation

            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.4
            //# An endpoint MAY send up to two full-sized datagrams containing
            //# ack-eliciting packets, to avoid an expensive consecutive PTO
            //# expiration due to a single lost datagram or transmit data from
            //# multiple packet number spaces.

            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.4
            //# Sending two packets on PTO
            //# expiration increases resilience to packet drops, thus reducing the
            //# probability of consecutive PTO events.
            let transmission_count = if packets_in_flight { 2 } else { 1 };

            self.state = PtoState::RequiresTransmission(transmission_count);
            true
        } else {
            false
        }
    }

    /// Queries the component for any outgoing frames that need to get sent
    pub fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        match self.state {
            PtoState::RequiresTransmission(0) => self.state = PtoState::Idle,
            PtoState::RequiresTransmission(remaining) => {
                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.4
                //# When there is no data to send, the sender SHOULD send
                //# a PING or other ack-eliciting frame in a single packet, re-arming the
                //# PTO timer.

                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.2.1
                //# When the PTO fires, the client MUST send a Handshake packet if it
                //# has Handshake keys, otherwise it MUST send an Initial packet in a
                //# UDP datagram with a payload of at least 1200 bytes.

                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#A.9
                //# // Client sends an anti-deadlock packet: Initial is padded
                //# // to earn more anti-amplification credit,
                //# // a Handshake packet proves address ownership.

                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.4
                //# All probe packets sent on a PTO MUST be ack-eliciting.

                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.5
                //# Probe packets MUST NOT be blocked by the congestion controller.

                // The early transmission will automatically ensure all initial packets sent by the
                // client are padded to 1200 bytes
                if context.ack_elicitation().is_ack_eliciting()
                    || context.write_frame_forced(&frame::Ping).is_some()
                {
                    let remaining = remaining - 1;
                    self.state = if remaining == 0 {
                        PtoState::Idle
                    } else {
                        PtoState::RequiresTransmission(remaining)
                    };
                }
            }
            _ => {}
        }
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.1
    //# A sender SHOULD restart its PTO timer every time an ack-eliciting
    //# packet is sent or acknowledged, when the handshake is confirmed
    //# (Section 4.1.2 of [QUIC-TLS]), or when Initial or Handshake keys are
    //# discarded (Section 4.9 of [QUIC-TLS]).
    pub fn update(&mut self, base_timestamp: Timestamp, pto_period: Duration) {
        self.timer.set(base_timestamp + pto_period);
    }

    /// Cancels the PTO timer
    pub fn cancel(&mut self) {
        self.timer.cancel();
    }
}

impl transmission::interest::Provider for Pto {
    fn transmission_interest(&self) -> transmission::Interest {
        if matches!(self.state, PtoState::RequiresTransmission(_)) {
            transmission::Interest::Forced
        } else {
            transmission::Interest::None
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        contexts::testing::{MockWriteContext, OutgoingFrameBuffer},
        path::{self, Path},
        recovery::{
            context::mock::MockContext, manager::Manager, pto::PtoState::RequiresTransmission,
        },
    };
    use core::time::Duration;
    use s2n_quic_core::{
        connection, endpoint,
        frame::ack_elicitation::AckElicitation,
        packet::number::PacketNumberSpace,
        recovery::{
            congestion_controller::testing::unlimited::CongestionController as Unlimited,
            RttEstimator, K_GRANULARITY,
        },
        varint::VarInt,
    };

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.2
    //= type=test
    //# When no previous RTT is available, the initial RTT
    //# SHOULD be set to 333ms, resulting in a 1 second initial timeout, as
    //# recommended in [RFC6298].
    #[test]
    fn one_second_pto_when_no_previous_rtt_available() {
        let space = PacketNumberSpace::Handshake;
        let max_ack_delay = Duration::from_millis(0);
        let mut manager = Manager::new(space, max_ack_delay);
        let now = s2n_quic_platform::time::now();

        let path = Path::new(
            Default::default(),
            connection::PeerId::TEST_ID,
            RttEstimator::new(max_ack_delay),
            Unlimited::default(),
            false,
        );

        manager
            .pto
            .update(now, path.rtt_estimator.pto_period(path.pto_backoff, space));

        assert!(manager.pto.timer.is_armed());
        assert_eq!(
            manager.pto.timer.iter().next(),
            Some(now + Duration::from_millis(999))
        );
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.2.1
    //= type=test
    //# That is,
    //# the client MUST set the probe timer if the client has not received an
    //# acknowledgement for one of its Handshake packets and the handshake is
    //# not confirmed (see Section 4.1.2 of [QUIC-TLS]), even if there are no
    //# packets in flight.
    #[test]
    fn pto_armed_if_handshake_not_confirmed() {
        let space = PacketNumberSpace::Handshake;
        let mut manager = Manager::new(space, Duration::from_millis(10));
        let now = s2n_quic_platform::time::now() + Duration::from_secs(10);
        let is_handshake_confirmed = false;

        let mut path = Path::new(
            Default::default(),
            connection::PeerId::TEST_ID,
            RttEstimator::new(Duration::from_millis(10)),
            Unlimited::default(),
            false,
        );

        path.on_validated();

        manager.update_pto_timer(&path, now, is_handshake_confirmed);

        assert!(manager.pto.timer.is_armed());
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.1
    //= type=test
    //# The PTO period MUST be at least kGranularity, to avoid the timer
    //# expiring immediately.
    #[test]
    fn pto_must_be_at_least_k_granularity() {
        let space = PacketNumberSpace::Handshake;
        let max_ack_delay = Duration::from_millis(0);
        let mut manager = Manager::new(space, max_ack_delay);
        let now = s2n_quic_platform::time::now();

        let mut path = Path::new(
            Default::default(),
            connection::PeerId::TEST_ID,
            RttEstimator::new(max_ack_delay),
            Unlimited::default(),
            false,
        );

        // Update RTT with the smallest possible sample
        path.rtt_estimator.update_rtt(
            Duration::from_millis(0),
            Duration::from_nanos(1),
            now,
            true,
            space,
        );

        manager
            .pto
            .update(now, path.rtt_estimator.pto_period(path.pto_backoff, space));

        assert!(manager.pto.timer.is_armed());
        assert!(manager.pto.timer.iter().next().unwrap() >= now + K_GRANULARITY);
    }

    #[test]
    fn timers() {
        let space = PacketNumberSpace::ApplicationData;
        let mut manager = Manager::new(space, Duration::from_millis(10));
        let pto_time = s2n_quic_platform::time::now() + Duration::from_secs(10);

        // No timer is set
        assert_eq!(manager.timers().count(), 0);

        // PTO timer is armed
        manager.pto.timer.set(pto_time);
        assert_eq!(manager.timers().count(), 1);
        assert_eq!(manager.timers().next(), Some(pto_time));
    }

    #[test]
    fn on_transmit() {
        let space = PacketNumberSpace::ApplicationData;
        let mut manager = Manager::new(space, Duration::from_millis(10));
        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut context = MockWriteContext::new(
            s2n_quic_platform::time::now(),
            &mut frame_buffer,
            transmission::Constraint::CongestionLimited, // Recovery manager ignores constraints
            endpoint::Type::Client,
        );

        // Already idle
        manager.pto.state = PtoState::Idle;
        manager.on_transmit(&mut context);
        assert_eq!(manager.pto.state, PtoState::Idle);

        // No transmissions required
        manager.pto.state = RequiresTransmission(0);
        manager.on_transmit(&mut context);
        assert_eq!(manager.pto.state, PtoState::Idle);

        // One transmission required, not ack eliciting
        manager.pto.state = RequiresTransmission(1);
        context.write_frame_forced(&frame::Padding { length: 1 });
        assert!(!context.ack_elicitation().is_ack_eliciting());
        manager.on_transmit(&mut context);
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.4
        //= type=test
        //# All probe packets sent on a PTO MUST be ack-eliciting.

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.4
        //= type=test
        //# When the PTO timer expires, an ack-eliciting packet MUST be sent.

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.4
        //= type=test
        //# When there is no data to send, the sender SHOULD send
        //# a PING or other ack-eliciting frame in a single packet, re-arming the
        //# PTO timer.
        assert!(context.ack_elicitation().is_ack_eliciting());
        assert_eq!(manager.pto.state, PtoState::Idle);

        // One transmission required, ack eliciting
        manager.pto.state = RequiresTransmission(1);
        context.write_frame_forced(&frame::Ping);
        manager.on_transmit(&mut context);
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.4
        //= type=test
        //# All probe packets sent on a PTO MUST be ack-eliciting.

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.4
        //= type=test
        //# When the PTO timer expires, an ack-eliciting packet MUST be sent.
        assert!(context.ack_elicitation().is_ack_eliciting());
        assert_eq!(manager.pto.state, PtoState::Idle);

        // Two transmissions required
        manager.pto.state = RequiresTransmission(2);
        manager.on_transmit(&mut context);
        assert_eq!(manager.pto.state, RequiresTransmission(1));
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.5
    //= type=test
    //# A sender MUST however count these packets as being additionally in
    //# flight, since these packets add network load without establishing
    //# packet loss.
    #[test]
    fn probe_packets_count_towards_bytes_in_flight() {
        let space = PacketNumberSpace::ApplicationData;
        let mut manager = Manager::new(space, Duration::from_millis(10));

        manager.pto.state = PtoState::RequiresTransmission(2);

        let mut context = MockContext::default();
        let outcome = transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: 100,
            packet_number: space.new_packet_number(VarInt::from_u8(1)),
        };
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(1)),
            outcome,
            s2n_quic_platform::time::now(),
            path::Id::new(0),
            &mut context,
        );

        assert_eq!(context.path.congestion_controller.bytes_in_flight, 100);
    }
}
