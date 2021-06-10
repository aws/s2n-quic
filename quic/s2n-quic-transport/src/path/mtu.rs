// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    contexts::{OnTransmitError, WriteContext},
    path::MINIMUM_MTU,
    timer::VirtualTimer,
    transmission,
    transmission::Interest,
};
use core::time::Duration;
use s2n_codec::EncoderValue;
use s2n_quic_core::{
    ack, frame, inet::SocketAddress, packet::number::PacketNumber, recovery::CongestionController,
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
    #[allow(dead_code)] // TODO: confirm if BASE is needed
    Base,
    // SEARCH_REQUESTED is used to indicate a probe packet has been requested
    // to be transmitted, but has not been transmitted yet.
    SearchRequested,
    //= https://tools.ietf.org/rfc/rfc8899.txt#5.2
    //# The SEARCHING state is the main probing state.
    Searching(PacketNumber, Timestamp),
    //= https://tools.ietf.org/rfc/rfc8899.txt#5.2
    //# The SEARCH_COMPLETE state indicates that a search has completed.
    SearchComplete,
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#14.3
//# Endpoints SHOULD set the initial value of BASE_PLPMTU (Section 5.1 of
//# [DPLPMTUD]) to be consistent with QUIC's smallest allowed maximum
//# datagram size.

//= https://tools.ietf.org/rfc/rfc8899.txt#5.1.2
//# When using IPv4, there is no currently equivalent size specified,
//# and a default BASE_PLPMTU of 1200 bytes is RECOMMENDED.
const BASE_PLPMTU: u16 = MINIMUM_MTU;

//= https://tools.ietf.org/rfc/rfc8899.txt#5.1.2
//# The MAX_PROBES is the maximum value of the PROBE_COUNT
//# counter (see Section 5.1.3).  MAX_PROBES represents the limit for
//# the number of consecutive probe attempts of any size.  Search
//# algorithms benefit from a MAX_PROBES value greater than 1 because
//# this can provide robustness to isolated packet loss.  The default
//# value of MAX_PROBES is 3.
const MAX_PROBES: u8 = 3;

//= https://tools.ietf.org/rfc/rfc894.txt
//# The minimum length of the data field of a packet sent over an
//# Ethernet is 1500 octets, thus the maximum length of an IP datagram
//# sent over an Ethernet is 1500 octets.
const ETHERNET_MTU: u16 = 1500;

//= https://tools.ietf.org/rfc/rfc768.txt
//# Length  is the length  in octets  of this user datagram  including  this
//# header  and the data.   (This  means  the minimum value of the length is
//# eight.)
const UPD_HEADER_LEN: u16 = 8;

// IPv4 header ranges from 20-60 bytes, depending on Options
const IPV4_MIN_HEADER_LEN: u16 = 20;
// IPv6 header is always 40 bytes, plus extensions
const IPV6_MIN_HEADER_LEN: u16 = 40;

// If the next value to probe is within the PROBE_THRESHOLD bytes of
// the current Path MTU, probing will be considered complete.
const PROBE_THRESHOLD: u16 = 20;

//= https://tools.ietf.org/rfc/rfc8899.txt#5.1.1
//# The PMTU_RAISE_TIMER is configured to the period a
//# sender will continue to use the current PLPMTU, after which it
//# reenters the Search Phase.  This timer has a period of 600
//# seconds, as recommended by PLPMTUD [RFC4821].
const PMTU_RAISE_TIMER_DURATION: Duration = Duration::from_secs(600);

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
    // The maximum size datagram to probe for. In contrast to the max_plpmtu,
    // this value will decrease if probes are not acknowledged.
    max_probe_size: u16,
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
    //# The PMTU_RAISE_TIMER is configured to the period a
    //# sender will continue to use the current PLPMTU, after which it
    //# reenters the Search Phase.
    pmtu_raise_timer: VirtualTimer,
}

// TODO: Remove when used
#[allow(dead_code)]
impl Controller {
    /// Construct a new mtu::Controller with the given `max_plpmtu` and `peer_socket_address`
    pub fn new(max_plpmtu: u16, peer_socket_address: SocketAddress) -> Self {
        debug_assert!(
            max_plpmtu >= BASE_PLPMTU,
            "max_plpmtu must be at least {}",
            BASE_PLPMTU
        );

        let min_ip_header_len = match peer_socket_address {
            SocketAddress::IpV4(_) => IPV4_MIN_HEADER_LEN,
            SocketAddress::IpV6(_) => IPV6_MIN_HEADER_LEN,
        };

        // The most likely MTU is based on standard Ethernet MTU minus the minimum length
        // IP headers (without IPv4 options or IPv6 extensions) and UPD header
        let initial_probed_size =
            (ETHERNET_MTU - UPD_HEADER_LEN - min_ip_header_len).min(max_plpmtu);

        Self {
            state: State::Disabled,
            plpmtu: BASE_PLPMTU,
            max_plpmtu,
            max_probe_size: max_plpmtu,
            probe_count: 0,
            probed_size: initial_probed_size,
            pmtu_raise_timer: VirtualTimer::default(),
        }
    }

    /// Enable path MTU probing
    pub fn enable(&mut self) {
        debug_assert_eq!(self.state, State::Disabled);

        // TODO: Look up current MTU in a cache. If there is a cache hit
        //       move directly to SearchComplete and arm the PMTU raise timer.
        //       Otherwise, start searching for a larger PMTU immediately
        self.request_new_search();
    }

    /// Returns all timers for the component
    pub fn timers(&self) -> impl Iterator<Item = Timestamp> {
        self.pmtu_raise_timer.iter()
    }

    /// Called when the connection timer expires
    pub fn on_timeout(&mut self, now: Timestamp) {
        if self.pmtu_raise_timer.poll_expiration(now).is_ready() {
            // Reset the max_probe_size to the max_plpmtu to allow for larger
            // probe sizes
            self.max_probe_size = self.max_plpmtu;
            // send another probe
            self.request_new_search();
        }
    }

    /// This method gets called when a packet delivery got acknowledged
    pub fn on_packet_ack<A: ack::Set, CC: CongestionController>(
        &mut self,
        ack_set: &A,
        mut congestion_controller: CC,
    ) {
        if let State::Searching(packet_number, transmit_time) = self.state {
            if ack_set.contains(packet_number) {
                self.plpmtu = self.probed_size;
                // A new MTU has been confirmed, notify the congestion controller
                congestion_controller.on_mtu_update(self.plpmtu);

                self.probed_size = self.next_probe_size();

                if self.plpmtu + PROBE_THRESHOLD > self.probed_size {
                    // The next probe size is within the threshold of the current MTU
                    // so its not worth additional probing.
                    self.state = State::SearchComplete;
                    self.pmtu_raise_timer
                        .set(transmit_time + PMTU_RAISE_TIMER_DURATION);
                } else {
                    self.request_new_search();
                }
            }
        }
    }

    /// This method gets called when a packet loss is reported
    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        if let State::Searching(packet_number, _) = self.state {
            if ack_set.contains(packet_number) {
                if self.probe_count == MAX_PROBES {
                    // We've sent MAX_PROBES without acknowledgement, so
                    // attempt a smaller probe size
                    self.max_probe_size = self.probed_size;
                    self.probed_size = self.next_probe_size();
                    self.request_new_search();
                } else {
                    // Try the same probe size again
                    self.state = State::SearchRequested
                }
            }
        }
    }

    /// Queries the component for any outgoing frames that need to get sent
    pub fn on_transmit<W: WriteContext>(&mut self, context: &mut W) -> Result<(), OnTransmitError> {
        if !matches!(self.state, State::SearchRequested) {
            return Ok(());
        }

        let header_len = 0; // TODO: add header_len to write context
        let probe_payload_size = self.probed_size as usize - header_len;
        if context.remaining_capacity() <= probe_payload_size {
            // There isn't enough capacity in the buffer to write the datagram we
            // want to probe, so we've reached the maximum pmtu and the search is complete.
            self.state = State::SearchComplete;
            return Ok(());
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#14.4
        //# Endpoints could limit the content of PMTU probes to PING and PADDING
        //# frames, since packets that are larger than the current maximum
        //# datagram size are more likely to be dropped by the network.

        context.write_frame(&frame::Ping);
        let padding_size = probe_payload_size - &frame::Ping.encoding_size();
        if let Some(packet_number) = context.write_frame(&frame::Padding {
            length: padding_size,
        }) {
            self.probe_count += 1;
            self.state = State::Searching(packet_number, context.current_time());
        }

        Ok(())
    }

    /// Gets the currently validated maximum transmission unit
    pub fn mtu(&self) -> usize {
        self.plpmtu as usize
    }

    /// Calculates the next MTU size to probe for, based on a binary search
    fn next_probe_size(&self) -> u16 {
        self.plpmtu + ((self.max_probe_size - self.plpmtu) / 2)
    }

    /// Requests a new search to be initiated
    fn request_new_search(&mut self) {
        self.state = State::SearchRequested;
        self.probe_count = 0;
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
