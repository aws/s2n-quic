// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    contexts::{OnTransmitError, WriteContext},
    path::{Path, MINIMUM_MTU},
    timer::VirtualTimer,
    transmission,
    transmission::{interest::Provider, Interest},
};
use core::time::Duration;
use s2n_codec::{Encoder, EncoderBuffer, EncoderValue};
use s2n_quic_core::{
    ack, frame,
    inet::{ExplicitCongestionNotification, SocketAddress},
    io::tx,
    packet::number::PacketNumber,
    recovery::CongestionController,
    time::Timestamp,
};

//= https://tools.ietf.org/rfc/rfc8899.txt#5.2
//#    |         |
//#    | Start   | PL indicates loss
//#    |         |  of connectivity
//#    v         v
//# +---------------+                                   +---------------+
//# |    DISABLED   |                                   |     ERROR     |
//# +---------------+               PROBE_TIMER expiry: +---------------+
//#         | PL indicates     PROBE_COUNT = MAX_PROBES or    ^      |
//#         | connectivity  PTB: PL_PTB_SIZE < BASE_PLPMTU    |      |
//#         +--------------------+         +------------------+      |
//#                              |         |                         |
//#                              v         |       BASE_PLPMTU Probe |
//#                           +---------------+          acked       |
//#                           |      BASE     |--------------------->+
//#                           +---------------+                      |
//#                              ^ |    ^  ^                         |
//#          Black hole detected | |    |  | Black hole detected     |
//#         +--------------------+ |    |  +--------------------+    |
//#         |                      +----+                       |    |
//#         |                PROBE_TIMER expiry:                |    |
//#         |             PROBE_COUNT < MAX_PROBES              |    |
//#         |                                                   |    |
//#         |               PMTU_RAISE_TIMER expiry             |    |
//#         |    +-----------------------------------------+    |    |
//#         |    |                                         |    |    |
//#         |    |                                         v    |    v
//# +---------------+                                   +---------------+
//# |SEARCH_COMPLETE|                                   |   SEARCHING   |
//# +---------------+                                   +---------------+
//#    |    ^    ^                                         |    |    ^
//#    |    |    |                                         |    |    |
//#    |    |    +-----------------------------------------+    |    |
//#    |    |            MAX_PLPMTU Probe acked or              |    |
//#    |    |  PROBE_TIMER expiry: PROBE_COUNT = MAX_PROBES or  |    |
//#    +----+            PTB: PL_PTB_SIZE = PLPMTU              +----+
//# CONFIRMATION_TIMER expiry:                        PROBE_TIMER expiry:
//# PROBE_COUNT < MAX_PROBES or               PROBE_COUNT < MAX_PROBES or
//#      PLPMTU Probe acked                           Probe acked or PTB:
//#                                    PLPMTU < PL_PTB_SIZE < PROBED_SIZE
#[derive(Debug, PartialEq, Eq)]
enum State {
    //= https://tools.ietf.org/rfc/rfc8899.txt#5.2
    //# The DISABLED state is the initial state before probing has started.
    Disabled,
    //= https://tools.ietf.org/rfc/rfc8899.txt#5.2
    //# The BASE state is used to confirm that the BASE_PLPMTU size is
    //# supported by the network path and is designed to allow an
    //# application to continue working when there are transient reductions
    //# in the actual PMTU.
    Base,
    SearchRequested,
    //= https://tools.ietf.org/rfc/rfc8899.txt#5.2
    //# The SEARCHING state is the main probing state.
    Searching(PacketNumber),
    //= https://tools.ietf.org/rfc/rfc8899.txt#5.2
    //# The SEARCH_COMPLETE state indicates that a search has completed.
    SearchComplete,
    //= https://tools.ietf.org/rfc/rfc8899.txt#5.2
    //# The ERROR state represents the case where either the network path
    //# is not known to support a PLPMTU of at least the BASE_PLPMTU size
    //# or when there is contradictory information about the network path
    //# that would otherwise result in excessive variation in the MPS
    //# signaled to the higher layer.
    Error,
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#14.3
//# Endpoints SHOULD set the initial value of BASE_PLPMTU (Section 5.1 of
//# [DPLPMTUD]) to be consistent with QUIC's smallest allowed maximum
//# datagram size.

//= https://tools.ietf.org/rfc/rfc8899.txt#5.1.2
//# When using
//# IPv4, there is no currently equivalent size specified, and a
//# default BASE_PLPMTU of 1200 bytes is RECOMMENDED.
const BASE_PLPMTU: u16 = MINIMUM_MTU;

//= https://tools.ietf.org/rfc/rfc8899.txt#5.1.2
//# The MAX_PROBES is the maximum value of the PROBE_COUNT
//# counter (see Section 5.1.3).  MAX_PROBES represents the limit for
//# the number of consecutive probe attempts of any size.  Search
//# algorithms benefit from a MAX_PROBES value greater than 1 because
//# this can provide robustness to isolated packet loss.  The default
//# value of MAX_PROBES is 3.
const MAX_PROBES: u8 = 3;

#[derive(Debug)]
pub struct Controller {
    state: State,
    //= https://tools.ietf.org/rfc/rfc8899.txt#2
    //# The Packetization Layer PMTU is an estimate of the largest size
    //# of PL datagram that can be sent by a path, controlled by PLPMTUD
    plpmtu: u16,
    //= https://tools.ietf.org/rfc/rfc8899.txt#5.1.2
    //# The MAX_PLPMTU is the largest size of PLPMTU.
    max_plpmtu: u16,
    //= https://tools.ietf.org/rfc/rfc8899.txt#5.1.3
    //# The PROBE_COUNT is a count of the number of successive
    //# unsuccessful probe packets that have been sent.
    probe_count: u8,
    //= https://tools.ietf.org/rfc/rfc8899.txt#5.1.3
    //# The PROBED_SIZE is the size of the current probe packet
    //# as determined at the PL.  This is a tentative value for the
    //# PLPMTU, which is awaiting confirmation by an acknowledgment.
    probed_size: u16,
    //= https://tools.ietf.org/rfc/rfc8899.txt#5.1.1
    //# The PROBE_TIMER is configured to expire after a period
    //# longer than the maximum time to receive an acknowledgment to a
    //# probe packet.
    probe_timer: VirtualTimer,
    //= https://tools.ietf.org/rfc/rfc8899.txt#5.1.1
    //# The PMTU_RAISE_TIMER is configured to the period a
    //# sender will continue to use the current PLPMTU, after which it
    //# reenters the Search Phase.
    pmtu_raise_timer: VirtualTimer,
}

impl Controller {
    pub fn new(max_plpmtu: u16) -> Self {
        Self {
            state: State::Disabled,
            plpmtu: BASE_PLPMTU,
            max_plpmtu,
            probe_count: 0,
            probed_size: 1450, // TODO: use correct value
            probe_timer: VirtualTimer::default(),
            pmtu_raise_timer: VirtualTimer::default(),
        }
    }

    pub fn enable(&mut self) {
        debug_assert_eq!(self.state, State::Disabled);

        // TODO: Look up current MTU in the cache. If there is a cache hit
        //       move directly to SearchComplete[?]. Otherwise, we should start searching
        //       for a larger PMTU immediately
        self.state = State::SearchRequested;
    }

    /// Returns all timers for the component
    pub fn timers(&self) -> impl Iterator<Item = Timestamp> {
        core::iter::empty()
            .chain(self.probe_timer.iter())
            .chain(self.pmtu_raise_timer.iter())
    }

    /// Called when the connection timer expires
    pub fn on_timeout(&mut self, now: Timestamp) {
        if self.probe_timer.poll_expiration(now).is_ready() {
            self.probe_count += 1;
            if self.probe_count < MAX_PROBES {
                // send another probe
                self.state = State::SearchRequested;
            } else {
                self.state = State::SearchComplete;
            }
        }

        if self.pmtu_raise_timer.poll_expiration(now).is_ready() {
            // send another probe
            self.state = State::SearchRequested;
        }
    }

    /// This method gets called when a packet delivery got acknowledged
    pub fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) {
        if let State::Searching(packet_number) = self.state {
            if ack_set.contains(packet_number) {
                self.probe_count = 0;
                self.plpmtu = self.probed_size;
                self.probed_size += 1; // look up next probed size from table
            }
        }
    }

    /// This method gets called when a packet loss is reported
    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        if let State::Searching(packet_number) = self.state {
            if ack_set.contains(packet_number) {
                // An explicit "Lost" state is not used since PMTU Probe packets do not need
                // prioritization over other packets when lost
                self.state = State::SearchRequested;
            }
        }
    }

    /// Queries the component for any outgoing frames that need to get sent
    pub fn transmission<'a, CC: CongestionController>(
        &'a mut self,
        path: &'a mut Path<CC>,
    ) -> Transmission<'a, CC> {
        debug_assert!(
            !self.transmission_interest().is_none(),
            "transmission should only be called when transmission interest is expressed"
        );

        Transmission {
            path,
            mtu: self.probed_size,
        }
    }
}

impl transmission::interest::Provider for Controller {
    fn transmission_interest(&self) -> Interest {
        match self.state {
            State::SearchRequested => transmission::Interest::NewData,
            _ => transmission::Interest::None,
        }
    }
}

pub struct Transmission<'a, CC: CongestionController> {
    path: &'a mut Path<CC>,
    mtu: u16,
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

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#14.4
    //# Endpoints could limit the content of PMTU probes to PING and PADDING
    //# frames, since packets that are larger than the current maximum
    //# datagram size are more likely to be dropped by the network.
    fn write_payload(&mut self, buffer: &mut [u8]) -> usize {
        let mut buffer = EncoderBuffer::new(buffer);

        debug_assert!(self.mtu as usize <= buffer.capacity());

        frame::Ping.encode(&mut buffer);

        let padding_size = self.mtu as usize - buffer.len();

        frame::Padding {
            length: padding_size,
        }
        .encode(&mut buffer);

        let len = buffer.len();

        self.path.on_bytes_transmitted(len);

        len
    }
}
