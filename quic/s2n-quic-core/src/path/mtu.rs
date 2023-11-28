// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    counter::{Counter, Saturating},
    ensure, event,
    event::{builder::MtuUpdatedCause, IntoEvent},
    frame,
    inet::SocketAddress,
    packet::number::PacketNumber,
    path,
    path::{MaxMtu, IPV4_MIN_HEADER_LEN, IPV6_MIN_HEADER_LEN, MINIMUM_MTU, UDP_HEADER_LEN},
    recovery::{congestion_controller, CongestionController},
    time::{timer, Timer, Timestamp},
    transmission,
};
use core::time::Duration;
use s2n_codec::EncoderValue;

#[cfg(test)]
mod tests;

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use super::*;
    use crate::inet::{IpV4Address, SocketAddressV4};

    /// Creates a new mtu::Controller with an IPv4 address and the given `max_mtu`
    pub fn new_controller(max_mtu: u16) -> Controller {
        let ip = IpV4Address::new([127, 0, 0, 1]);
        let addr = SocketAddress::IpV4(SocketAddressV4::new(ip, 443));
        Controller::new(max_mtu.try_into().unwrap(), &addr)
    }

    /// Creates a new mtu::Controller with the given mtu and probed size
    pub fn test_controller(mtu: u16, probed_size: u16) -> Controller {
        let mut controller = new_controller(u16::max_value());
        controller.plpmtu = mtu;
        controller.probed_size = probed_size;
        controller
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum State {
    //= https://www.rfc-editor.org/rfc/rfc8899#section-5.2
    //# The DISABLED state is the initial state before probing has started.
    Disabled,
    /// SEARCH_REQUESTED is used to indicate a probe packet has been requested
    /// to be transmitted, but has not been transmitted yet.
    SearchRequested,
    //= https://www.rfc-editor.org/rfc/rfc8899#section-5.2
    //# The SEARCHING state is the main probing state.
    Searching(PacketNumber, Timestamp),
    //= https://www.rfc-editor.org/rfc/rfc8899#section-5.2
    //# The SEARCH_COMPLETE state indicates that a search has completed.
    SearchComplete,
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-14.3
//# Endpoints SHOULD set the initial value of BASE_PLPMTU (Section 5.1 of
//# [DPLPMTUD]) to be consistent with QUIC's smallest allowed maximum
//# datagram size.

//= https://www.rfc-editor.org/rfc/rfc8899#section-5.1.2
//# When using IPv4, there is no currently equivalent size specified,
//# and a default BASE_PLPMTU of 1200 bytes is RECOMMENDED.
const BASE_PLPMTU: u16 = MINIMUM_MTU;

//= https://www.rfc-editor.org/rfc/rfc8899#section-5.1.2
//# The MAX_PROBES is the maximum value of the PROBE_COUNT
//# counter (see Section 5.1.3).  MAX_PROBES represents the limit for
//# the number of consecutive probe attempts of any size.  Search
//# algorithms benefit from a MAX_PROBES value greater than 1 because
//# this can provide robustness to isolated packet loss.  The default
//# value of MAX_PROBES is 3.
const MAX_PROBES: u8 = 3;

/// The minimum length of the data field of a packet sent over an
/// Ethernet is 1500 octets, thus the maximum length of an IP datagram
/// sent over an Ethernet is 1500 octets.
/// See https://www.rfc-editor.org/rfc/rfc894.txt
const ETHERNET_MTU: u16 = 1500;

/// If the next value to probe is within the PROBE_THRESHOLD bytes of
/// the current Path MTU, probing will be considered complete.
const PROBE_THRESHOLD: u16 = 20;

/// When the black_hole_counter exceeds this threshold, on_black_hole_detected will be
/// called to reduce the MTU to the BASE_PLPMTU. The black_hole_counter is incremented when
/// a burst of consecutive packets is lost that starts with a packet that is:
///      1) not an MTU probe
///      2) larger than the BASE_PLPMTU
///      3) sent after the largest MTU-sized acknowledged packet number
/// This is a possible indication that the path cannot support the MTU that was previously confirmed.
const BLACK_HOLE_THRESHOLD: u8 = 3;

/// After a black hole has been detected, the mtu::Controller will wait this duration
/// before probing for a larger MTU again.
const BLACK_HOLE_COOL_OFF_DURATION: Duration = Duration::from_secs(60);

//= https://www.rfc-editor.org/rfc/rfc8899#section-5.1.1
//# The PMTU_RAISE_TIMER is configured to the period a
//# sender will continue to use the current PLPMTU, after which it
//# reenters the Search Phase.  This timer has a period of 600
//# seconds, as recommended by PLPMTUD [RFC4821].
const PMTU_RAISE_TIMER_DURATION: Duration = Duration::from_secs(600);

#[derive(Clone, Debug)]
pub struct Controller {
    state: State,
    //= https://www.rfc-editor.org/rfc/rfc8899#section-2
    //# The Packetization Layer PMTU is an estimate of the largest size
    //# of PL datagram that can be sent by a path, controlled by PLPMTUD
    plpmtu: u16,
    /// The maximum size any packet can reach
    max_mtu: MaxMtu,
    /// The maximum size the UDP payload can reach for any probe packet.
    max_udp_payload: u16,
    //= https://www.rfc-editor.org/rfc/rfc8899#section-5.1.3
    //# The PROBED_SIZE is the size of the current probe packet
    //# as determined at the PL.  This is a tentative value for the
    //# PLPMTU, which is awaiting confirmation by an acknowledgment.
    probed_size: u16,
    /// The maximum size datagram to probe for. In contrast to the max_udp_payload,
    /// this value will decrease if probes are not acknowledged.
    max_probe_size: u16,
    //= https://www.rfc-editor.org/rfc/rfc8899#section-5.1.3
    //# The PROBE_COUNT is a count of the number of successive
    //# unsuccessful probe packets that have been sent.
    probe_count: u8,
    /// A count of the number of packets with a size > MINIMUM_MTU lost since
    /// the last time a packet with size equal to the current MTU was acknowledged.
    black_hole_counter: Counter<u8, Saturating>,
    /// The largest acknowledged packet with size >= the plpmtu. Used when tracking
    /// packets that have been lost for the purpose of detecting a black hole.
    largest_acked_mtu_sized_packet: Option<PacketNumber>,
    //= https://www.rfc-editor.org/rfc/rfc8899#section-5.1.1
    //# The PMTU_RAISE_TIMER is configured to the period a
    //# sender will continue to use the current PLPMTU, after which it
    //# reenters the Search Phase.
    pmtu_raise_timer: Timer,
}

impl Controller {
    /// Construct a new mtu::Controller with the given `max_mtu` and `peer_socket_address`
    ///
    /// The UDP header length and IP header length will be subtracted from `max_mtu` to
    /// determine the max_udp_payload used for limiting the payload length of probe packets.
    /// max_mtu is the maximum allowed mtu, e.g. for jumbo frames this value is expected to
    /// be over 9000.
    #[inline]
    pub fn new(max_mtu: MaxMtu, peer_socket_address: &SocketAddress) -> Self {
        let min_ip_header_len = match peer_socket_address {
            SocketAddress::IpV4(_) => IPV4_MIN_HEADER_LEN,
            SocketAddress::IpV6(_) => IPV6_MIN_HEADER_LEN,
        };
        let max_udp_payload =
            (u16::from(max_mtu) - UDP_HEADER_LEN - min_ip_header_len).max(BASE_PLPMTU);

        // The UDP payload size for the most likely MTU is based on standard Ethernet MTU minus
        // the minimum length IP headers (without IPv4 options or IPv6 extensions) and UPD header
        let initial_probed_size =
            (ETHERNET_MTU - UDP_HEADER_LEN - min_ip_header_len).min(max_udp_payload);

        Self {
            state: State::Disabled,
            plpmtu: BASE_PLPMTU,
            probed_size: initial_probed_size,
            max_mtu,
            max_udp_payload,
            max_probe_size: max_udp_payload,
            probe_count: 0,
            black_hole_counter: Default::default(),
            largest_acked_mtu_sized_packet: None,
            pmtu_raise_timer: Timer::default(),
        }
    }

    /// Enable path MTU probing
    #[inline]
    pub fn enable(&mut self) {
        // ensure we haven't already enabled the controller
        ensure!(self.state == State::Disabled);

        // TODO: Look up current MTU in a cache. If there is a cache hit
        //       move directly to SearchComplete and arm the PMTU raise timer.
        //       Otherwise, start searching for a larger PMTU immediately
        self.request_new_search(None);
    }

    /// Called when the connection timer expires
    #[inline]
    pub fn on_timeout(&mut self, now: Timestamp) {
        ensure!(self.pmtu_raise_timer.poll_expiration(now).is_ready());
        self.request_new_search(None);
    }

    //= https://www.rfc-editor.org/rfc/rfc8899#section-4.2
    //# When
    //# supported, this mechanism MAY also be used by DPLPMTUD to acknowledge
    //# reception of a probe packet.
    /// This method gets called when a packet delivery got acknowledged
    #[inline]
    pub fn on_packet_ack<CC: CongestionController, Pub: event::ConnectionPublisher>(
        &mut self,
        packet_number: PacketNumber,
        sent_bytes: u16,
        congestion_controller: &mut CC,
        path_id: path::Id,
        publisher: &mut Pub,
    ) {
        // no need to process anything in the disabled state
        ensure!(self.state != State::Disabled);

        // MTU probes are only sent in application data space
        ensure!(packet_number.space().is_application_data());

        if sent_bytes >= self.plpmtu
            && self
                .largest_acked_mtu_sized_packet
                .map_or(true, |pn| packet_number > pn)
        {
            // Reset the black hole counter since a packet the size of the current MTU or larger
            // has been acknowledged, indicating the path can still support the current MTU
            self.black_hole_counter = Default::default();
            self.largest_acked_mtu_sized_packet = Some(packet_number);
        }

        if let State::Searching(probe_packet_number, transmit_time) = self.state {
            if packet_number == probe_packet_number {
                self.plpmtu = self.probed_size;
                // A new MTU has been confirmed, notify the congestion controller
                congestion_controller.on_mtu_update(
                    self.plpmtu,
                    &mut congestion_controller::PathPublisher::new(publisher, path_id),
                );

                publisher.on_mtu_updated(event::builder::MtuUpdated {
                    path_id: path_id.into_event(),
                    mtu: self.plpmtu,
                    cause: MtuUpdatedCause::ProbeAcknowledged,
                });

                self.update_probed_size();

                //= https://www.rfc-editor.org/rfc/rfc8899#section-8
                //# To avoid excessive load, the interval between individual probe
                //# packets MUST be at least one RTT, and the interval between rounds of
                //# probing is determined by the PMTU_RAISE_TIMER.

                // Subsequent probe packets are sent based on the round trip transmission and
                // acknowledgement/loss of a packet, so the interval will be at least 1 RTT.
                self.request_new_search(Some(transmit_time));
            }
        }
    }

    //= https://www.rfc-editor.org/rfc/rfc8899#section-3
    //# The PL is REQUIRED to be
    //# robust in the case where probe packets are lost due to other
    //# reasons (including link transmission error, congestion).
    /// This method gets called when a packet loss is reported
    #[allow(clippy::too_many_arguments)]
    #[inline]
    pub fn on_packet_loss<CC: CongestionController, Pub: event::ConnectionPublisher>(
        &mut self,
        packet_number: PacketNumber,
        lost_bytes: u16,
        new_loss_burst: bool,
        now: Timestamp,
        congestion_controller: &mut CC,
        path_id: path::Id,
        publisher: &mut Pub,
    ) {
        // MTU probes are only sent in application data space
        ensure!(packet_number.space().is_application_data());

        match &self.state {
            State::Disabled => {}
            State::Searching(probe_pn, _) if *probe_pn == packet_number => {
                // The MTU probe was lost
                if self.probe_count == MAX_PROBES {
                    // We've sent MAX_PROBES without acknowledgement, so
                    // attempt a smaller probe size
                    self.max_probe_size = self.probed_size;
                    self.update_probed_size();
                    self.request_new_search(None);
                } else {
                    // Try the same probe size again
                    self.state = State::SearchRequested
                }
            }
            State::Searching(_, _) | State::SearchComplete | State::SearchRequested => {
                if (BASE_PLPMTU + 1..=self.plpmtu).contains(&lost_bytes)
                    && self
                        .largest_acked_mtu_sized_packet
                        .map_or(true, |pn| packet_number > pn)
                    && new_loss_burst
                {
                    // A non-probe packet larger than the BASE_PLPMTU that was sent after the last
                    // acknowledged MTU-sized packet has been lost
                    self.black_hole_counter += 1;
                }

                if self.black_hole_counter > BLACK_HOLE_THRESHOLD {
                    self.on_black_hole_detected(now, congestion_controller, path_id, publisher);
                }
            }
        }
    }

    /// Gets the currently validated maximum transmission unit, not including IP or UDP header len
    #[inline]
    pub fn mtu(&self) -> usize {
        self.plpmtu as usize
    }

    /// Returns the maximum size any packet can reach
    #[inline]
    pub fn max_mtu(&self) -> MaxMtu {
        self.max_mtu
    }

    /// Gets the MTU currently being probed for
    #[inline]
    pub fn probed_sized(&self) -> usize {
        self.probed_size as usize
    }

    /// Sets `probed_size` to the next MTU size to probe for based on a binary search
    #[inline]
    fn update_probed_size(&mut self) {
        //= https://www.rfc-editor.org/rfc/rfc8899#section-5.3.2
        //# Implementations SHOULD select the set of probe packet sizes to
        //# maximize the gain in PLPMTU from each search step.
        self.probed_size = self.plpmtu + ((self.max_probe_size - self.plpmtu) / 2);
    }

    /// Requests a new search to be initiated
    ///
    /// If `last_probe_time` is supplied, the PMTU Raise Timer will be armed as
    /// necessary if the probed_size is already within the PROBE_THRESHOLD
    /// of the current PLPMTU
    #[inline]
    fn request_new_search(&mut self, last_probe_time: Option<Timestamp>) {
        if self.probed_size - self.plpmtu >= PROBE_THRESHOLD {
            self.probe_count = 0;
            self.state = State::SearchRequested;
        } else {
            // The next probe size is within the threshold of the current MTU
            // so its not worth additional probing.
            self.state = State::SearchComplete;

            if let Some(last_probe_time) = last_probe_time {
                self.arm_pmtu_raise_timer(last_probe_time + PMTU_RAISE_TIMER_DURATION);
            }
        }
    }

    /// Called when an excessive number of packets larger than the BASE_PLPMTU have been lost
    #[inline]
    fn on_black_hole_detected<CC: CongestionController, Pub: event::ConnectionPublisher>(
        &mut self,
        now: Timestamp,
        congestion_controller: &mut CC,
        path_id: path::Id,
        publisher: &mut Pub,
    ) {
        self.black_hole_counter = Default::default();
        self.largest_acked_mtu_sized_packet = None;
        // Reset the plpmtu back to the BASE_PLPMTU and notify the congestion controller
        self.plpmtu = BASE_PLPMTU;
        congestion_controller.on_mtu_update(
            BASE_PLPMTU,
            &mut congestion_controller::PathPublisher::new(publisher, path_id),
        );
        // Cancel any current probes
        self.state = State::SearchComplete;
        // Arm the PMTU raise timer to try a larger MTU again after a cooling off period
        self.arm_pmtu_raise_timer(now + BLACK_HOLE_COOL_OFF_DURATION);

        publisher.on_mtu_updated(event::builder::MtuUpdated {
            path_id: path_id.into_event(),
            mtu: self.plpmtu,
            cause: MtuUpdatedCause::Blackhole,
        })
    }

    /// Arm the PMTU Raise Timer if there is still room to increase the
    /// MTU before hitting the max plpmtu
    #[inline]
    fn arm_pmtu_raise_timer(&mut self, timestamp: Timestamp) {
        // Reset the max_probe_size to the max_udp_payload to allow for larger probe sizes
        self.max_probe_size = self.max_udp_payload;
        self.update_probed_size();

        if self.probed_size - self.plpmtu >= PROBE_THRESHOLD {
            // There is still some room to try a larger MTU again,
            // so arm the pmtu raise timer
            self.pmtu_raise_timer.set(timestamp);
        }
    }
}

impl timer::Provider for Controller {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        self.pmtu_raise_timer.timers(query)?;

        Ok(())
    }
}

impl transmission::Provider for Controller {
    /// Queries the component for any outgoing frames that need to get sent
    ///
    /// This method assumes that no other data (other than the packet header) has been written
    /// to the supplied `WriteContext`. This necessitates the caller ensuring the probe packet
    /// written by this method to be in its own connection transmission.
    #[inline]
    fn on_transmit<W: transmission::Writer>(&mut self, context: &mut W) {
        //= https://www.rfc-editor.org/rfc/rfc8899#section-5.2
        //# When used with an acknowledged PL (e.g., SCTP), DPLPMTUD SHOULD NOT continue to
        //# generate PLPMTU probes in this state.
        ensure!(self.state == State::SearchRequested);

        ensure!(context.transmission_mode().is_mtu_probing());

        // Each packet contains overhead in the form of a packet header and an authentication tag.
        // This overhead contributes to the overall size of the packet, so the payload we write
        // to the packet will account for this overhead to reach the target probed size.
        let probe_payload_size =
            self.probed_size as usize - context.header_len() - context.tag_len();

        if context.remaining_capacity() < probe_payload_size {
            // There isn't enough capacity in the buffer to write the datagram we
            // want to probe, so we've reached the maximum pmtu and the search is complete.
            self.state = State::SearchComplete;
            return;
        }

        //= https://www.rfc-editor.org/rfc/rfc9000#section-14.4
        //# Endpoints could limit the content of PMTU probes to PING and PADDING
        //# frames, since packets that are larger than the current maximum
        //# datagram size are more likely to be dropped by the network.

        //= https://www.rfc-editor.org/rfc/rfc8899#section-3
        //# Probe loss recovery: It is RECOMMENDED to use probe packets that
        //# do not carry any user data that would require retransmission if
        //# lost.

        //= https://www.rfc-editor.org/rfc/rfc8899#section-4.1
        //# DPLPMTUD MAY choose to use only one of these methods to simplify the
        //# implementation.

        context.write_frame(&frame::Ping);
        let padding_size = probe_payload_size - frame::Ping.encoding_size();
        if let Some(packet_number) = context.write_frame(&frame::Padding {
            length: padding_size,
        }) {
            self.probe_count += 1;
            self.state = State::Searching(packet_number, context.current_time());
        }
    }
}

impl transmission::interest::Provider for Controller {
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        match self.state {
            State::SearchRequested => query.on_new_data(),
            _ => Ok(()),
        }
    }
}
