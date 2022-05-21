// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    contexts::WriteContext,
    path::{MaxMtu, MINIMUM_MTU},
    transmission,
};
use core::time::Duration;
use s2n_codec::EncoderValue;
use s2n_quic_core::{
    counter::{Counter, Saturating},
    frame,
    inet::SocketAddress,
    packet::number::PacketNumber,
    path::{IPV4_MIN_HEADER_LEN, IPV6_MIN_HEADER_LEN, UDP_HEADER_LEN},
    recovery::CongestionController,
    time::{timer, Timer, Timestamp},
};

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
/// a packet is lost that is:
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
    pub fn new(max_mtu: MaxMtu, peer_socket_address: &SocketAddress) -> Self {
        let min_ip_header_len = match peer_socket_address.unmap() {
            SocketAddress::IpV4(_) => IPV4_MIN_HEADER_LEN,
            SocketAddress::IpV6(_) => IPV6_MIN_HEADER_LEN,
        };
        let max_udp_payload = u16::from(max_mtu) - UDP_HEADER_LEN - min_ip_header_len;
        debug_assert!(
            max_udp_payload >= BASE_PLPMTU,
            "max_udp_payload must be at least {}",
            BASE_PLPMTU
        );

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
    pub fn enable(&mut self) {
        if self.state != State::Disabled {
            return;
        }

        // TODO: Look up current MTU in a cache. If there is a cache hit
        //       move directly to SearchComplete and arm the PMTU raise timer.
        //       Otherwise, start searching for a larger PMTU immediately
        self.request_new_search(None);
    }

    /// Called when the connection timer expires
    pub fn on_timeout(&mut self, now: Timestamp) {
        if self.pmtu_raise_timer.poll_expiration(now).is_ready() {
            self.request_new_search(None);
        }
    }

    //= https://www.rfc-editor.org/rfc/rfc8899#section-4.2
    //# When
    //# supported, this mechanism MAY also be used by DPLPMTUD to acknowledge
    //# reception of a probe packet.
    /// This method gets called when a packet delivery got acknowledged
    pub fn on_packet_ack<CC: CongestionController>(
        &mut self,
        packet_number: PacketNumber,
        sent_bytes: u16,
        congestion_controller: &mut CC,
    ) {
        if self.state == State::Disabled || !packet_number.space().is_application_data() {
            return;
        }

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
                congestion_controller.on_mtu_update(self.plpmtu);

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
    pub fn on_packet_loss<CC: CongestionController>(
        &mut self,
        packet_number: PacketNumber,
        lost_bytes: u16,
        now: Timestamp,
        congestion_controller: &mut CC,
    ) {
        // MTU probes are only sent in application data space
        if !packet_number.space().is_application_data() {
            return;
        }

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
                {
                    // A non-probe packet larger than the BASE_PLPMTU that was sent after the last
                    // acknowledged MTU-sized packet has been lost
                    self.black_hole_counter += 1;
                }

                if self.black_hole_counter > BLACK_HOLE_THRESHOLD {
                    self.on_black_hole_detected(now, congestion_controller);
                }
            }
        }
    }

    /// Queries the component for any outgoing frames that need to get sent
    ///
    /// This method assumes that no other data (other than the packet header) has been written
    /// to the supplied `WriteContext`. This necessitates the caller ensuring the probe packet
    /// written by this method to be in its own connection transmission.
    pub fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        if self.state != State::SearchRequested || !context.transmission_mode().is_mtu_probing() {
            //= https://www.rfc-editor.org/rfc/rfc8899#section-5.2
            //# When used with an acknowledged PL (e.g., SCTP), DPLPMTUD SHOULD NOT continue to
            //# generate PLPMTU probes in this state.
            return;
        }

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

    /// Gets the currently validated maximum transmission unit, not including IP or UDP header len
    pub fn mtu(&self) -> usize {
        self.plpmtu as usize
    }

    /// Returns the maximum size any packet can reach
    pub fn max_mtu(&self) -> MaxMtu {
        self.max_mtu
    }

    /// Gets the MTU currently being probed for
    pub fn probed_sized(&self) -> usize {
        self.probed_size as usize
    }

    /// Sets `probed_size` to the next MTU size to probe for based on a binary search
    fn update_probed_size(&mut self) {
        //= https://www.rfc-editor.org/rfc/rfc8899#section-5.3.2
        //# Implementations SHOULD select the set of probe packet sizes to
        //# maximize the gain in PLPMTU from each search step.
        self.probed_size = self.plpmtu + ((self.max_probe_size - self.plpmtu) / 2)
    }

    /// Requests a new search to be initiated
    ///
    /// If `last_probe_time` is supplied, the PMTU Raise Timer will be armed as
    /// necessary if the probed_size is already within the PROBE_THRESHOLD
    /// of the current PLPMTU
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
    fn on_black_hole_detected<CC: CongestionController>(
        &mut self,
        now: Timestamp,
        congestion_controller: &mut CC,
    ) {
        self.black_hole_counter = Default::default();
        self.largest_acked_mtu_sized_packet = None;
        // Reset the plpmtu back to the BASE_PLPMTU and notify the congestion controller
        self.plpmtu = BASE_PLPMTU;
        congestion_controller.on_mtu_update(BASE_PLPMTU);
        // Cancel any current probes
        self.state = State::SearchComplete;
        // Arm the PMTU raise timer to try a larger MTU again after a cooling off period
        self.arm_pmtu_raise_timer(now + BLACK_HOLE_COOL_OFF_DURATION);
    }

    /// Arm the PMTU Raise Timer if there is still room to increase the
    /// MTU before hitting the max plpmtu
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

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use crate::path::mtu::{test, Controller};

    /// Creates a new mtu::Controller with the given mtu and probed size
    pub fn test_controller(mtu: u16, probed_size: u16) -> Controller {
        let mut controller = test::new_controller(u16::max_value());
        controller.plpmtu = mtu;
        controller.probed_size = probed_size;
        controller
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::contexts::testing::{MockWriteContext, OutgoingFrameBuffer};
    use s2n_quic_core::{
        endpoint, frame::Frame, packet::number::PacketNumberSpace,
        recovery::congestion_controller::testing::mock::CongestionController,
        time::timer::Provider as _, varint::VarInt,
    };
    use s2n_quic_platform::time::now;
    use std::{convert::TryInto, net::SocketAddr};

    /// Creates a new mtu::Controller with an IPv4 address and the given `max_mtu`
    pub fn new_controller(max_mtu: u16) -> Controller {
        let addr: SocketAddr = "127.0.0.1:443".parse().unwrap();
        Controller::new(max_mtu.try_into().unwrap(), &addr.into())
    }

    /// Creates an application space packet number with the given value
    fn pn(nr: usize) -> PacketNumber {
        PacketNumberSpace::ApplicationData.new_packet_number(VarInt::new(nr as u64).unwrap())
    }

    #[test]
    fn base_plpmtu_is_1200() {
        //= https://www.rfc-editor.org/rfc/rfc8899#section-5.1.2
        //= type=test
        //# When using
        //# IPv4, there is no currently equivalent size specified, and a
        //# default BASE_PLPMTU of 1200 bytes is RECOMMENDED.
        assert_eq!(BASE_PLPMTU, 1200);
    }

    #[test]
    #[should_panic]
    fn new_max_mtu_too_small() {
        new_controller(BASE_PLPMTU + UDP_HEADER_LEN + IPV4_MIN_HEADER_LEN - 1);
    }

    #[test]
    fn new_max_mtu_smaller_than_common_mtu() {
        let mut controller = new_controller(BASE_PLPMTU + UDP_HEADER_LEN + IPV4_MIN_HEADER_LEN + 1);
        assert_eq!(BASE_PLPMTU + 1, controller.probed_size);

        controller.enable();
        assert_eq!(State::SearchComplete, controller.state);
    }

    #[test]
    fn new_ipv4() {
        let addr: SocketAddr = "127.0.0.1:443".parse().unwrap();
        let controller = Controller::new(1600.try_into().unwrap(), &addr.into());
        assert_eq!(
            1600 - UDP_HEADER_LEN - IPV4_MIN_HEADER_LEN,
            controller.max_udp_payload
        );
        assert_eq!(
            1600 - UDP_HEADER_LEN - IPV4_MIN_HEADER_LEN,
            controller.max_probe_size
        );
        assert_eq!(BASE_PLPMTU as usize, controller.mtu());
        assert_eq!(0, controller.probe_count);
        assert_eq!(State::Disabled, controller.state);
        assert!(!controller.pmtu_raise_timer.is_armed());
        assert_eq!(
            ETHERNET_MTU - UDP_HEADER_LEN - IPV4_MIN_HEADER_LEN,
            controller.probed_size
        );
    }

    #[test]
    fn new_ipv6() {
        let addr: SocketAddr = "[2001:0db8:85a3:0001:0002:8a2e:0370:7334]:9000"
            .parse()
            .unwrap();
        let controller = Controller::new(2000.try_into().unwrap(), &addr.into());
        assert_eq!(
            2000 - UDP_HEADER_LEN - IPV6_MIN_HEADER_LEN,
            controller.max_udp_payload
        );
        assert_eq!(
            2000 - UDP_HEADER_LEN - IPV6_MIN_HEADER_LEN,
            controller.max_probe_size
        );
        assert_eq!(BASE_PLPMTU as usize, controller.mtu());
        assert_eq!(0, controller.probe_count);
        assert_eq!(State::Disabled, controller.state);
        assert!(!controller.pmtu_raise_timer.is_armed());
        assert_eq!(
            ETHERNET_MTU - UDP_HEADER_LEN - IPV6_MIN_HEADER_LEN,
            controller.probed_size
        );
    }

    #[test]
    fn new_ipv6_mapped() {
        let addr: SocketAddr = "127.0.0.1:443".parse().unwrap();
        let addr: SocketAddress = addr.into();
        let controller = Controller::new(1600.try_into().unwrap(), &addr.to_ipv6_mapped().into());

        assert_eq!(
            1600 - UDP_HEADER_LEN - IPV4_MIN_HEADER_LEN,
            controller.max_udp_payload
        );
        assert_eq!(
            1600 - UDP_HEADER_LEN - IPV4_MIN_HEADER_LEN,
            controller.max_probe_size
        );
        assert_eq!(
            ETHERNET_MTU - UDP_HEADER_LEN - IPV4_MIN_HEADER_LEN,
            controller.probed_size
        );
    }

    #[test]
    fn enable_already_enabled() {
        let mut controller = new_controller(1500);
        assert_eq!(State::Disabled, controller.state);
        controller.enable();
        assert_eq!(State::SearchRequested, controller.state);
        controller.state = State::SearchComplete;
        controller.enable();
        assert_eq!(State::SearchComplete, controller.state);
    }

    #[test]
    fn enable() {
        let mut controller = new_controller(1500);
        assert_eq!(State::Disabled, controller.state);
        controller.enable();
        assert_eq!(State::SearchRequested, controller.state);
    }

    //= https://www.rfc-editor.org/rfc/rfc8899#section-4.2
    //= type=test
    //# When
    //# supported, this mechanism MAY also be used by DPLPMTUD to acknowledge
    //# reception of a probe packet.
    #[test]
    fn on_packet_ack_within_threshold() {
        let mut controller = new_controller(1472 + PROBE_THRESHOLD * 2);
        let max_udp_payload = controller.max_udp_payload;
        let pn = pn(1);
        let mut cc = CongestionController::default();
        let now = now();
        controller.state = State::Searching(pn, now);
        controller.probed_size = BASE_PLPMTU;
        controller.max_probe_size = BASE_PLPMTU + PROBE_THRESHOLD * 2 - 1;

        controller.on_packet_ack(pn, BASE_PLPMTU, &mut cc);

        assert_eq!(
            BASE_PLPMTU + (max_udp_payload - BASE_PLPMTU) / 2,
            controller.probed_size
        );
        assert_eq!(1, cc.on_mtu_update);
        assert_eq!(State::SearchComplete, controller.state);
        assert!(controller.pmtu_raise_timer.is_armed());
        assert_eq!(
            Some(now + PMTU_RAISE_TIMER_DURATION),
            controller.next_expiration()
        );

        // Enough time passes that its time to try raising the PMTU again
        let now = now + PMTU_RAISE_TIMER_DURATION;
        controller.on_timeout(now);

        assert_eq!(State::SearchRequested, controller.state);
        assert_eq!(
            BASE_PLPMTU + (max_udp_payload - BASE_PLPMTU) / 2,
            controller.probed_size
        );
    }

    #[test]
    fn on_packet_ack_within_threshold_of_max_plpmtu() {
        let mut controller = new_controller(1472 + (PROBE_THRESHOLD * 2 - 1));
        let max_udp_payload = controller.max_udp_payload;
        let pn = pn(1);
        let mut cc = CongestionController::default();
        let now = now();
        controller.state = State::Searching(pn, now);

        controller.on_packet_ack(pn, controller.probed_size, &mut cc);

        assert_eq!(1472 + (max_udp_payload - 1472) / 2, controller.probed_size);
        assert_eq!(1, cc.on_mtu_update);
        assert_eq!(State::SearchComplete, controller.state);
        assert!(!controller.pmtu_raise_timer.is_armed());
    }

    //= https://www.rfc-editor.org/rfc/rfc8899#section-5.3.2
    //= type=test
    //# Implementations SHOULD select the set of probe packet sizes to
    //# maximize the gain in PLPMTU from each search step.

    //= https://www.rfc-editor.org/rfc/rfc8899#section-8
    //= type=test
    //# To avoid excessive load, the interval between individual probe
    //# packets MUST be at least one RTT, and the interval between rounds of
    //# probing is determined by the PMTU_RAISE_TIMER.
    #[test]
    fn on_packet_ack_search_requested() {
        let mut controller = new_controller(1500 + (PROBE_THRESHOLD * 2));
        let max_udp_payload = controller.max_udp_payload;
        let pn = pn(1);
        let mut cc = CongestionController::default();
        let now = now();
        controller.state = State::Searching(pn, now);

        controller.on_packet_ack(pn, controller.probed_size, &mut cc);

        assert_eq!(1472 + (max_udp_payload - 1472) / 2, controller.probed_size);
        assert_eq!(1, cc.on_mtu_update);
        assert_eq!(State::SearchRequested, controller.state);
        assert!(!controller.pmtu_raise_timer.is_armed());
    }

    #[test]
    fn on_packet_ack_resets_black_hole_counter() {
        let mut controller = new_controller(1500 + (PROBE_THRESHOLD * 2));
        let pnum = pn(3);
        let mut cc = CongestionController::default();
        controller.enable();

        controller.black_hole_counter += 1;
        // ack a packet smaller than the plpmtu
        controller.on_packet_ack(pnum, controller.plpmtu - 1, &mut cc);
        assert_eq!(controller.black_hole_counter, 1);
        assert_eq!(None, controller.largest_acked_mtu_sized_packet);

        // ack a packet the size of the plpmtu
        controller.on_packet_ack(pnum, controller.plpmtu, &mut cc);
        assert_eq!(controller.black_hole_counter, 0);
        assert_eq!(Some(pnum), controller.largest_acked_mtu_sized_packet);

        controller.black_hole_counter += 1;

        // ack an older packet
        let pnum_2 = pn(2);
        controller.on_packet_ack(pnum_2, controller.plpmtu, &mut cc);
        assert_eq!(controller.black_hole_counter, 1);
        assert_eq!(Some(pnum), controller.largest_acked_mtu_sized_packet);
    }

    #[test]
    fn on_packet_ack_disabled_controller() {
        let mut controller = new_controller(1500 + (PROBE_THRESHOLD * 2));
        let pnum = pn(3);
        let mut cc = CongestionController::default();

        controller.black_hole_counter += 1;
        controller.largest_acked_mtu_sized_packet = Some(pnum);

        let pn = pn(10);
        controller.on_packet_ack(pn, controller.plpmtu, &mut cc);

        assert_eq!(State::Disabled, controller.state);
        assert_eq!(controller.black_hole_counter, 1);
        assert_eq!(Some(pnum), controller.largest_acked_mtu_sized_packet);
    }

    #[test]
    fn on_packet_ack_not_application_space() {
        let mut controller = new_controller(1500 + (PROBE_THRESHOLD * 2));
        let pnum = pn(3);
        let mut cc = CongestionController::default();
        controller.enable();

        controller.black_hole_counter += 1;
        controller.largest_acked_mtu_sized_packet = Some(pnum);

        // on_packet_ack will be called with packet numbers from Initial and Handshake space,
        // so it should not fail in this scenario.
        let pn = PacketNumberSpace::Handshake.new_packet_number(VarInt::from_u8(10));
        controller.on_packet_ack(pn, controller.plpmtu, &mut cc);
        assert_eq!(controller.black_hole_counter, 1);
        assert_eq!(Some(pnum), controller.largest_acked_mtu_sized_packet);
    }

    //= https://www.rfc-editor.org/rfc/rfc8899#section-3
    //= type=test
    //# The PL is REQUIRED to be
    //# robust in the case where probe packets are lost due to other
    //# reasons (including link transmission error, congestion).
    #[test]
    fn on_packet_loss() {
        let mut controller = new_controller(1500);
        let max_udp_payload = controller.max_udp_payload;
        let pn = pn(1);
        let mut cc = CongestionController::default();
        let now = now();
        controller.state = State::Searching(pn, now);
        let probed_size = controller.probed_size;

        controller.on_packet_loss(pn, controller.probed_size, now, &mut cc);

        assert_eq!(0, cc.on_mtu_update);
        assert_eq!(max_udp_payload, controller.max_probe_size);
        assert_eq!(probed_size, controller.probed_size);
        assert_eq!(State::SearchRequested, controller.state);
    }

    #[test]
    fn on_packet_loss_max_probes() {
        let mut controller = new_controller(1500);
        let max_udp_payload = controller.max_udp_payload;
        let pn = pn(1);
        let mut cc = CongestionController::default();
        let now = now();
        controller.state = State::Searching(pn, now);
        controller.probe_count = MAX_PROBES;
        assert_eq!(max_udp_payload, controller.max_probe_size);

        controller.on_packet_loss(pn, controller.probed_size, now, &mut cc);

        assert_eq!(0, cc.on_mtu_update);
        assert_eq!(1472, controller.max_probe_size);
        assert_eq!(
            BASE_PLPMTU + (1472 - BASE_PLPMTU) / 2,
            controller.probed_size
        );
        assert_eq!(State::SearchRequested, controller.state);
    }

    #[test]
    fn on_packet_loss_black_hole() {
        let mut controller = new_controller(1500);
        let mut cc = CongestionController::default();
        let now = now();
        controller.plpmtu = 1472;
        controller.enable();

        for i in 0..BLACK_HOLE_THRESHOLD + 1 {
            let pn = pn(i as usize);

            // Losing a packet the size of the BASE_PLPMTU should not increase the black_hole_counter
            controller.on_packet_loss(pn, BASE_PLPMTU, now, &mut cc);
            assert_eq!(controller.black_hole_counter, i);

            // Losing a packet larger than the PLPMTU should not increase the black_hole_counter
            controller.on_packet_loss(pn, controller.plpmtu + 1, now, &mut cc);
            assert_eq!(controller.black_hole_counter, i);

            controller.on_packet_loss(pn, BASE_PLPMTU + 1, now, &mut cc);
            if i < BLACK_HOLE_THRESHOLD {
                assert_eq!(controller.black_hole_counter, i + 1);
            }
        }

        assert_eq!(controller.black_hole_counter, 0);
        assert_eq!(None, controller.largest_acked_mtu_sized_packet);
        assert_eq!(1, cc.on_mtu_update);
        assert_eq!(BASE_PLPMTU, controller.plpmtu);
        assert_eq!(State::SearchComplete, controller.state);
        assert_eq!(
            Some(now + BLACK_HOLE_COOL_OFF_DURATION),
            controller.pmtu_raise_timer.next_expiration()
        );
    }

    #[test]
    fn on_packet_loss_disabled_controller() {
        let mut controller = new_controller(1500);
        let mut cc = CongestionController::default();
        let now = now();

        for i in 0..BLACK_HOLE_THRESHOLD + 1 {
            let pn = pn(i as usize);
            assert_eq!(controller.black_hole_counter, 0);
            controller.on_packet_loss(pn, BASE_PLPMTU + 1, now, &mut cc);
        }

        assert_eq!(State::Disabled, controller.state);
        assert_eq!(controller.black_hole_counter, 0);
        assert_eq!(0, cc.on_mtu_update);
    }

    #[test]
    fn on_packet_loss_not_application_space() {
        let mut controller = new_controller(1500);
        let mut cc = CongestionController::default();

        // test the loss in each state
        for state in vec![
            State::Disabled,
            State::SearchRequested,
            State::Searching(pn(1), now()),
            State::SearchComplete,
        ] {
            controller.state = state;
            for i in 0..BLACK_HOLE_THRESHOLD + 1 {
                // on_packet_loss may be called with packet numbers from Initial and Handshake space
                // so it should not fail in this scenario.
                let pn = PacketNumberSpace::Initial.new_packet_number(VarInt::from_u8(i));
                controller.on_packet_loss(pn, BASE_PLPMTU + 1, now(), &mut cc);
                assert_eq!(controller.black_hole_counter, 0);
                assert_eq!(0, cc.on_mtu_update);
            }
        }
    }

    //= https://www.rfc-editor.org/rfc/rfc8899#section-5.2
    //= type=test
    //# When used with an
    //# acknowledged PL (e.g., SCTP), DPLPMTUD SHOULD NOT continue to
    //# generate PLPMTU probes in this state.
    #[test]
    fn on_transmit_search_not_requested() {
        let mut controller = new_controller(1500);
        controller.state = State::SearchComplete;
        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut write_context = MockWriteContext::new(
            now(),
            &mut frame_buffer,
            transmission::Constraint::None,
            transmission::Mode::MtuProbing,
            endpoint::Type::Server,
        );

        controller.on_transmit(&mut write_context);
        assert!(frame_buffer.is_empty());
        assert_eq!(State::SearchComplete, controller.state);
    }

    #[test]
    fn on_transmit_not_mtu_probing() {
        let mut controller = new_controller(1500);
        controller.state = State::SearchRequested;
        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut write_context = MockWriteContext::new(
            now(),
            &mut frame_buffer,
            transmission::Constraint::None,
            transmission::Mode::Normal,
            endpoint::Type::Server,
        );

        controller.on_transmit(&mut write_context);
        assert!(frame_buffer.is_empty());
        assert_eq!(State::SearchRequested, controller.state);

        controller.state = State::SearchComplete;
        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut write_context = MockWriteContext::new(
            now(),
            &mut frame_buffer,
            transmission::Constraint::None,
            transmission::Mode::Normal,
            endpoint::Type::Server,
        );

        controller.on_transmit(&mut write_context);
        assert!(frame_buffer.is_empty());
        assert_eq!(State::SearchComplete, controller.state);
    }

    #[test]
    fn on_transmit_no_capacity() {
        let mut controller = new_controller(1500);
        controller.state = State::SearchRequested;
        let mut frame_buffer = OutgoingFrameBuffer::new();
        frame_buffer.set_max_packet_size(Some(controller.probed_size as usize - 1));
        let mut write_context = MockWriteContext::new(
            now(),
            &mut frame_buffer,
            transmission::Constraint::None,
            transmission::Mode::MtuProbing,
            endpoint::Type::Server,
        );

        controller.on_transmit(&mut write_context);
        assert!(frame_buffer.is_empty());
        assert_eq!(State::SearchComplete, controller.state);
    }

    //= https://www.rfc-editor.org/rfc/rfc9000#section-14.4
    //= type=test
    //# Endpoints could limit the content of PMTU probes to PING and PADDING
    //# frames, since packets that are larger than the current maximum
    //# datagram size are more likely to be dropped by the network.

    //= https://www.rfc-editor.org/rfc/rfc8899#section-3
    //= type=test
    //# Probe loss recovery: It is RECOMMENDED to use probe packets that
    //# do not carry any user data that would require retransmission if
    //# lost.

    //= https://www.rfc-editor.org/rfc/rfc8899#section-4.1
    //= type=test
    //# DPLPMTUD MAY choose to use only one of these methods to simplify the
    //# implementation.
    #[test]
    fn on_transmit() {
        let mut controller = new_controller(1500);
        controller.state = State::SearchRequested;
        let now = now();
        let mut frame_buffer = OutgoingFrameBuffer::new();
        frame_buffer.set_max_packet_size(Some(controller.probed_size as usize));
        let mut write_context = MockWriteContext::new(
            now,
            &mut frame_buffer,
            transmission::Constraint::None,
            transmission::Mode::MtuProbing,
            endpoint::Type::Server,
        );
        let packet_number = write_context.packet_number();

        controller.on_transmit(&mut write_context);
        assert_eq!(0, write_context.remaining_capacity());
        assert_eq!(
            Frame::Ping(frame::Ping),
            write_context.frame_buffer.pop_front().unwrap().as_frame()
        );
        assert_eq!(
            Frame::Padding(frame::Padding {
                length: controller.probed_size as usize - 1
            }),
            write_context.frame_buffer.pop_front().unwrap().as_frame()
        );
        assert_eq!(State::Searching(packet_number, now), controller.state);
    }
}
