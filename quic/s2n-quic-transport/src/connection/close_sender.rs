// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection::finalization,
    transmission::{self, interest::Provider as _},
};
use bytes::Bytes;
use core::{task::Poll, time::Duration};
use s2n_quic_core::{
    counter::{self, Counter},
    inet::{ExplicitCongestionNotification, SocketAddress},
    io::tx,
    path::Path,
    recovery::CongestionController,
    time::{Timer, Timestamp},
};

#[derive(Debug, Default)]
pub struct CloseSender {
    state: State,
}

impl CloseSender {
    pub fn close(&mut self, packet: Bytes, timeout: Duration, now: Timestamp) {
        debug_assert!(matches!(self.state, State::Idle));

        let mut close_timer = Timer::default();
        close_timer.set(now + timeout);

        self.state = State::Closing {
            packet,
            transmission: TransmissionState::Transmitting,
            close_timer,
            limiter: Limiter::default(),
        };
    }

    pub fn timers(&self) -> impl Iterator<Item = Timestamp> {
        self.state.timers()
    }

    pub fn on_timeout(&mut self, now: Timestamp) -> Poll<()> {
        self.state.on_timeout(now)
    }

    pub fn on_datagram_received(&mut self, rtt: Duration, now: Timestamp) {
        self.state.on_datagram_received(rtt, now);
    }

    pub fn transmission<'a, CC: CongestionController>(
        &'a mut self,
        path: &'a mut Path<CC>,
    ) -> Transmission<'a, CC> {
        debug_assert!(
            !self.transmission_interest().is_none(),
            "transmission should only be called when transmission interest is expressed"
        );

        if let State::Closing {
            packet,
            transmission,
            ..
        } = &mut self.state
        {
            Transmission {
                packet,
                transmission,
                path,
            }
        } else {
            unreachable!(
                "transmission should only be called when close sender has transmission interest"
            )
        }
    }
}

impl finalization::Provider for CloseSender {
    fn finalization_status(&self) -> finalization::Status {
        match &self.state {
            State::Idle => finalization::Status::Idle,
            State::Closing { .. } => finalization::Status::Draining,
            State::Closed => finalization::Status::Final,
        }
    }
}

impl transmission::interest::Provider for CloseSender {
    fn transmission_interest(&self) -> transmission::Interest {
        if matches!(
            self.state,
            State::Closing {
                transmission: TransmissionState::Transmitting,
                ..
            }
        ) {
            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-34.txt#3
            //# Packets containing frames besides ACK or CONNECTION_CLOSE frames
            //# count toward congestion control limits and are considered in-
            //# flight.

            // this packet only contains a CONNECTION_CLOSE so bypass the CC
            transmission::Interest::Forced
        } else {
            transmission::Interest::None
        }
    }
}

pub struct Transmission<'a, CC: CongestionController> {
    packet: &'a Bytes,
    transmission: &'a mut TransmissionState,
    path: &'a mut Path<CC>,
}

impl<'a, CC: CongestionController> tx::Message for Transmission<'a, CC> {
    fn remote_address(&mut self) -> SocketAddress {
        self.path.peer_socket_address
    }

    fn ecn(&mut self) -> ExplicitCongestionNotification {
        ExplicitCongestionNotification::default()
    }

    fn ipv6_flow_label(&mut self) -> u32 {
        0
    }

    fn delay(&mut self) -> Duration {
        Duration::default()
    }

    fn write_payload(&mut self, buffer: &mut [u8]) -> usize {
        let len = self.packet.len();

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-34.txt#10.2.1
        //# Note:  Allowing retransmission of a closing packet is an exception to
        //# the requirement that a new packet number be used for each packet
        //# in Section 12.3.  Sending new packet numbers is primarily of
        //# advantage to loss recovery and congestion control, which are not
        //# expected to be relevant for a closed connection.  Retransmitting
        //# the final packet requires less state.
        buffer[..len].copy_from_slice(self.packet);

        self.path.on_bytes_transmitted(len);
        *self.transmission = TransmissionState::Idle;

        len
    }
}

#[derive(Debug)]
enum State {
    Idle,
    Closing {
        packet: Bytes,
        limiter: Limiter,
        transmission: TransmissionState,
        close_timer: Timer,
    },
    Closed,
}

impl Default for State {
    fn default() -> Self {
        Self::Idle
    }
}

impl State {
    pub fn timers(&self) -> impl Iterator<Item = Timestamp> {
        if let Self::Closing {
            close_timer,
            limiter,
            ..
        } = self
        {
            Some(close_timer.iter().chain(limiter.timers()))
        } else {
            None
        }
        .into_iter()
        .flatten()
    }

    pub fn on_timeout(&mut self, now: Timestamp) -> Poll<()> {
        match self {
            Self::Idle => Poll::Pending,
            Self::Closing {
                close_timer,
                limiter,
                transmission,
                ..
            } => {
                if close_timer.poll_expiration(now).is_ready() {
                    *self = Self::Closed;
                    return Poll::Ready(());
                }

                if limiter.on_timeout(now).is_ready() {
                    *transmission = TransmissionState::Transmitting;
                }

                Poll::Pending
            }
            Self::Closed => Poll::Ready(()),
        }
    }

    pub fn on_datagram_received(&mut self, rtt: Duration, now: Timestamp) {
        if let Self::Closing { limiter, .. } = self {
            limiter.on_datagram_received(rtt, now);
        }
    }
}

#[derive(Debug)]
enum TransmissionState {
    Idle,
    Transmitting,
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-34.txt#10.2.1
//# An endpoint SHOULD limit the rate at which it generates packets in
//# the closing state.  For instance, an endpoint could wait for a
//# progressively increasing number of received packets or amount of time
//# before responding to received packets.
#[derive(Debug)]
struct Limiter {
    factor: Counter<u8, counter::Saturating>,
    received: Counter<u8, counter::Saturating>,
    debounce: Timer,
}

impl Default for Limiter {
    fn default() -> Self {
        Self {
            factor: Counter::new(1),
            received: Counter::new(0),
            debounce: Timer::default(),
        }
    }
}

impl Limiter {
    pub fn timers(&self) -> impl Iterator<Item = Timestamp> {
        self.debounce.iter()
    }

    pub fn on_timeout(&mut self, now: Timestamp) -> Poll<()> {
        self.debounce.poll_expiration(now)
    }

    pub fn on_datagram_received(&mut self, rtt: Duration, now: Timestamp) {
        if self.debounce.is_armed() {
            return;
        }

        self.received += 1;

        if self.received >= self.factor {
            self.received = Counter::new(0);
            self.factor += self.factor;
            self.debounce.set(now + rtt);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use s2n_quic_core::{
        io::tx::Message as _,
        path::{testing::test_path, MINIMUM_MTU},
        time::{testing::Clock, Clock as _},
    };

    static PACKET: Bytes = Bytes::from_static(b"CLOSE");

    #[test]
    fn model_test() {
        use bolero::generator::*;

        let close_time = (100..=3000).map_gen(Duration::from_millis);
        let rtt = close_time.clone().map_gen(|t| t / 3);
        let packet_durations = (0..=5000).map_gen(Duration::from_millis);
        let packet_sizes = 1..=9000;
        let events = gen::<Vec<_>>()
            .with()
            .values((packet_durations, packet_sizes));

        bolero::check!()
            .with_generator((close_time, rtt, events, gen()))
            .for_each(|(close_time, rtt, events, is_validated)| {
                let mut sender = CloseSender::default();
                let mut clock = Clock::default();
                let mut path = test_path();
                let mut buffer = [0; MINIMUM_MTU as usize];
                let mut transmission_count = 0usize;

                if *is_validated {
                    path.on_validated();
                } else {
                    // give the path some initial credits
                    path.on_bytes_received(MINIMUM_MTU as usize);
                }

                path.on_closing();
                sender.close(PACKET.clone(), *close_time, clock.get_time());

                // transmit an initial packet
                let interest = sender.transmission_interest();
                assert!(interest.can_transmit(path.transmission_constraint()));
                sender.transmission(&mut path).write_payload(&mut buffer);

                for (gap, packet_size) in events {
                    // get the next timer event
                    let gap = sender
                        .timers()
                        .map(|t| t - clock.get_time())
                        .chain(Some(*gap))
                        .min()
                        .unwrap();
                    clock.inc_by(gap);

                    // notify that we've received an incoming packet
                    sender.on_datagram_received(*rtt, clock.get_time());
                    path.on_bytes_received(*packet_size);

                    // try to send multiple times to ensure we only
                    // send a single packet
                    let transmission_count_before = transmission_count;
                    for _ in 0..3 {
                        let interest = sender.transmission_interest();
                        if interest.can_transmit(path.transmission_constraint()) {
                            sender.transmission(&mut path).write_payload(&mut buffer);
                            transmission_count += 1;
                        }
                    }
                    assert!(transmission_count - transmission_count_before <= 1);
                }

                // make sure we eventually clean up the sender
                clock.inc_by(*close_time);
                assert!(sender.on_timeout(clock.get_time()).is_ready());

                if events.is_empty() {
                    assert_eq!(transmission_count, 0);
                } else {
                    assert!(
                        transmission_count < events.len(),
                        "transmission count should never exceed the number of events"
                    );
                }
            })
    }

    #[test]
    fn limiter_test() {
        let mut limiter = Limiter::default();
        let mut clock = Clock::default();
        let rtt = Duration::from_millis(250);

        for count in (0..10).map(|v| 2usize.pow(v)) {
            for _ in 0..(count - 1) {
                limiter.on_datagram_received(rtt, clock.get_time());
            }

            // if count doesn't saturate the counter, make sure we're not armed
            if count < 256 {
                assert!(limiter.on_timeout(clock.get_time()).is_pending());
                limiter.on_datagram_received(rtt, clock.get_time());
            }

            assert!(limiter.on_timeout(clock.get_time()).is_pending());
            clock.inc_by(rtt);
            assert!(limiter.on_timeout(clock.get_time()).is_ready());
        }
    }
}
