// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    frame,
    time::{timer, Timer, Timestamp},
    transmission::{self, interest::Provider as _},
};
use core::{task::Poll, time::Duration};

#[derive(Debug, Default)]
pub struct Pto {
    timer: Timer,
    state: State,
}

impl Pto {
    /// Called when a timeout has occurred. Returns `Ready` if the PTO timer had expired.
    #[inline]
    pub fn on_timeout(&mut self, packets_in_flight: bool, timestamp: Timestamp) -> Poll<()> {
        ensure!(
            self.timer.poll_expiration(timestamp).is_ready(),
            Poll::Pending
        );

        //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.4
        //# When a PTO timer expires, a sender MUST send at least one ack-
        //# eliciting packet in the packet number space as a probe.

        //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.2.1
        //# Since the server could be blocked until more datagrams are received
        //# from the client, it is the client's responsibility to send packets to
        //# unblock the server until it is certain that the server has finished
        //# its address validation

        //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.4
        //# An endpoint
        //# MAY send up to two full-sized datagrams containing ack-eliciting
        //# packets to avoid an expensive consecutive PTO expiration due to a
        //# single lost datagram or to transmit data from multiple packet number
        //# spaces.

        //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.4
        //# Sending two packets on PTO
        //# expiration increases resilience to packet drops, thus reducing the
        //# probability of consecutive PTO events.
        let transmission_count = if packets_in_flight { 2 } else { 1 };

        self.state = State::RequiresTransmission(transmission_count);

        Poll::Ready(())
    }

    //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.1
    //# A sender SHOULD restart its PTO timer every time an ack-eliciting
    //# packet is sent or acknowledged, or when Initial or Handshake keys are
    //# discarded (Section 4.9 of [QUIC-TLS]).
    #[inline]
    pub fn update(&mut self, base_timestamp: Timestamp, pto_period: Duration) {
        self.timer.set(base_timestamp + pto_period);
    }

    /// Cancels the PTO timer
    #[inline]
    pub fn cancel(&mut self) {
        self.timer.cancel();
    }

    /// Returns the number of pending transmissions
    #[inline]
    pub fn transmissions(&self) -> u8 {
        self.state.transmissions()
    }

    #[inline]
    pub fn on_transmit_once(&mut self) {
        self.state.on_transmit();
    }

    #[inline]
    pub fn force_transmit(&mut self) {
        ensure!(matches!(self.state, State::Idle));
        self.state = State::RequiresTransmission(1);
    }
}

impl timer::Provider for Pto {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        self.timer.timers(query)?;
        Ok(())
    }
}

impl transmission::Provider for Pto {
    #[inline]
    fn on_transmit<W: transmission::Writer>(&mut self, context: &mut W) {
        // If we aren't currently in loss recovery probing mode, don't
        // send a probe. We could be in this state even if PtoState is
        // RequiresTransmission if we are just transmitting a ConnectionClose
        // frame.
        ensure!(context.transmission_mode().is_loss_recovery_probing());

        // Make sure we actually need to transmit
        ensure!(self.has_transmission_interest());

        //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.4
        //# All probe packets sent on a PTO MUST be ack-eliciting.
        if !context.ack_elicitation().is_ack_eliciting() {
            //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.4
            //# When there is no data to send, the sender SHOULD send
            //# a PING or other ack-eliciting frame in a single packet, re-arming the
            //# PTO timer.
            let frame = frame::Ping;

            //= https://www.rfc-editor.org/rfc/rfc9002#section-7.5
            //# Probe packets MUST NOT be blocked by the congestion controller.
            ensure!(context.write_frame_forced(&frame).is_some());
        }

        //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.2.1
        //# When the PTO fires, the client MUST send a Handshake packet if it
        //# has Handshake keys, otherwise it MUST send an Initial packet in a
        //# UDP datagram with a payload of at least 1200 bytes.

        //= https://www.rfc-editor.org/rfc/rfc9002#appendix-A.9
        //# // Client sends an anti-deadlock packet: Initial is padded
        //# // to earn more anti-amplification credit,
        //# // a Handshake packet proves address ownership.

        // The early transmission will automatically ensure all initial packets sent by the
        // client are padded to 1200 bytes

        self.on_transmit_once();
    }
}

impl transmission::interest::Provider for Pto {
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        if self.transmissions() > 0 {
            query.on_forced()?;
        }

        Ok(())
    }
}

#[derive(Debug, PartialEq)]
enum State {
    Idle,
    RequiresTransmission(u8),
}

impl Default for State {
    #[inline]
    fn default() -> Self {
        Self::Idle
    }
}

impl State {
    #[inline]
    fn transmissions(&self) -> u8 {
        match self {
            Self::Idle => 0,
            Self::RequiresTransmission(count) => *count,
        }
    }

    #[inline]
    fn on_transmit(&mut self) {
        match self {
            Self::Idle | Self::RequiresTransmission(0) => {
                debug_assert!(false, "transmitted pto in idle state");
            }
            Self::RequiresTransmission(1) => {
                *self = Self::Idle;
            }
            Self::RequiresTransmission(remaining) => {
                *remaining -= 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        endpoint,
        time::{Clock as _, NoopClock},
        transmission::{writer::testing, Provider as _, Writer as _},
    };

    #[test]
    fn on_transmit() {
        let clock = NoopClock;

        let mut frame_buffer = testing::OutgoingFrameBuffer::new();
        let mut context = testing::Writer::new(
            clock.get_time(),
            &mut frame_buffer,
            transmission::Constraint::CongestionLimited, // Recovery manager ignores constraints
            transmission::Mode::LossRecoveryProbing,
            endpoint::Type::Client,
        );

        let mut pto = Pto::default();

        // Already idle
        pto.on_transmit(&mut context);
        assert_eq!(pto.state, State::Idle);

        // No transmissions required
        pto.state = State::RequiresTransmission(0);
        pto.on_transmit(&mut context);
        assert_eq!(pto.state, State::RequiresTransmission(0));

        // One transmission required, not ack eliciting
        pto.state = State::RequiresTransmission(1);
        context.write_frame_forced(&frame::Padding { length: 1 });
        assert!(!context.ack_elicitation().is_ack_eliciting());
        pto.on_transmit(&mut context);

        //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.4
        //= type=test
        //# All probe packets sent on a PTO MUST be ack-eliciting.

        //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.4
        //= type=test
        //# When a PTO timer expires, a sender MUST send at least one ack-
        //# eliciting packet in the packet number space as a probe.

        //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.4
        //= type=test
        //# When there is no data to send, the sender SHOULD send
        //# a PING or other ack-eliciting frame in a single packet, re-arming the
        //# PTO timer.
        assert!(context.ack_elicitation().is_ack_eliciting());
        assert_eq!(pto.state, State::Idle);

        // One transmission required, ack eliciting
        pto.state = State::RequiresTransmission(1);
        context.write_frame_forced(&frame::Ping);
        pto.on_transmit(&mut context);
        //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.4
        //= type=test
        //# All probe packets sent on a PTO MUST be ack-eliciting.

        //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.4
        //= type=test
        //# When a PTO timer expires, a sender MUST send at least one ack-
        //# eliciting packet in the packet number space as a probe.
        assert!(context.ack_elicitation().is_ack_eliciting());
        assert_eq!(pto.state, State::Idle);

        // Two transmissions required
        pto.state = State::RequiresTransmission(2);
        pto.on_transmit(&mut context);
        assert_eq!(pto.state, State::RequiresTransmission(1));
    }

    #[test]
    fn on_transmit_normal_transmission_mode() {
        let clock = NoopClock;

        let mut frame_buffer = testing::OutgoingFrameBuffer::new();
        let mut context = testing::Writer::new(
            clock.get_time(),
            &mut frame_buffer,
            transmission::Constraint::CongestionLimited, // Recovery manager ignores constraints
            transmission::Mode::Normal,
            endpoint::Type::Client,
        );

        let mut pto = Pto {
            state: State::RequiresTransmission(2),
            ..Default::default()
        };

        pto.on_transmit(&mut context);
        assert_eq!(0, frame_buffer.frames.len());
        assert_eq!(pto.state, State::RequiresTransmission(2));
    }

    #[test]
    fn transmission_interest() {
        let mut pto = Pto::default();

        assert!(!pto.has_transmission_interest());

        pto.state = State::RequiresTransmission(0);
        assert!(!pto.has_transmission_interest());

        pto.state = State::RequiresTransmission(1);
        assert!(pto.has_transmission_interest());

        pto.state = State::RequiresTransmission(2);
        assert!(pto.has_transmission_interest());
    }
}
