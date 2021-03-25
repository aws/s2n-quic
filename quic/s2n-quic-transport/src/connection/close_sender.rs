// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{connection::finalization, transmission};
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
            backoff: Backoff::default(),
        };
    }

    pub fn timers(&self) -> impl Iterator<Item = &Timestamp> {
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
        backoff: Backoff,
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
    pub fn timers(&self) -> impl Iterator<Item = &Timestamp> {
        if let Self::Closing {
            close_timer,
            backoff,
            ..
        } = self
        {
            close_timer.iter().chain(backoff.timers())
        } else {
            None.iter().chain(None.iter())
        }
    }

    pub fn on_timeout(&mut self, now: Timestamp) -> Poll<()> {
        if let Self::Closing {
            close_timer,
            backoff,
            transmission,
            ..
        } = self
        {
            if close_timer.poll_expiration(now).is_ready() {
                *self = Self::Closed;
                return Poll::Ready(());
            }

            if backoff.on_timeout(now).is_ready() {
                *transmission = TransmissionState::Transmitting;
            }
        }

        Poll::Pending
    }

    pub fn on_datagram_received(&mut self, rtt: Duration, now: Timestamp) {
        if let Self::Closing { backoff, .. } = self {
            backoff.on_datagram_received(rtt, now);
        }
    }
}

#[derive(Debug)]
enum TransmissionState {
    Idle,
    Transmitting,
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
            transmission::Interest::NewData
        } else {
            transmission::Interest::None
        }
    }
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-34.txt#10.2.1
//# An endpoint SHOULD limit the rate at which it generates packets in
//# the closing state.  For instance, an endpoint could wait for a
//# progressively increasing number of received packets or amount of time
//# before responding to received packets.
#[derive(Debug)]
struct Backoff {
    factor: Counter<u8, counter::Saturating>,
    received: Counter<u8, counter::Saturating>,
    debounce: Timer,
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            factor: Counter::new(1),
            received: Counter::new(0),
            debounce: Timer::default(),
        }
    }
}

impl Backoff {
    pub fn timers(&self) -> core::option::Iter<Timestamp> {
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

    #[test]
    fn backoff_test() {
        let mut backoff = Backoff::default();
        let mut now = unsafe { Timestamp::from_duration(Duration::from_secs(1)) };
        let rtt = Duration::from_millis(250);

        for count in (0..10).map(|v| 2usize.pow(v)) {
            for _ in 0..(count - 1) {
                backoff.on_datagram_received(rtt, now);
            }

            // if count doesn't saturate the counter, make sure we're not armed
            if count < 256 {
                assert!(backoff.on_timeout(now).is_pending());
                backoff.on_datagram_received(rtt, now);
            }

            assert!(backoff.on_timeout(now).is_pending());
            now += rtt;
            assert!(backoff.on_timeout(now).is_ready());
        }
    }
}
