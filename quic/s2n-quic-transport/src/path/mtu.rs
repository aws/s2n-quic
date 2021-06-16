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

// The minimum length of the data field of a packet sent over an
// Ethernet is 1500 octets, thus the maximum length of an IP datagram
// sent over an Ethernet is 1500 octets.
// See https://tools.ietf.org/rfc/rfc894.txt
const ETHERNET_MTU: u16 = 1500;

// Length  is the length  in octets  of this user datagram  including  this
// header  and the data.   (This  means  the minimum value of the length is
// eight.)
// See https://tools.ietf.org/rfc/rfc768.txt
const UDP_HEADER_LEN: u16 = 8;

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
    //= https://tools.ietf.org/rfc/rfc8899.txt#5.1.3
    //# The PROBED_SIZE is the size of the current probe packet
    //# as determined at the PL.  This is a tentative value for the
    //# PLPMTU, which is awaiting confirmation by an acknowledgment.
    probed_size: u16,
    // The maximum size datagram to probe for. In contrast to the max_plpmtu,
    // this value will decrease if probes are not acknowledged.
    max_probe_size: u16,
    //= https://tools.ietf.org/rfc/rfc8899.txt#5.1.3
    //# The PROBE_COUNT is a count of the number of successive
    //# unsuccessful probe packets that have been sent.
    probe_count: u8,
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
            (ETHERNET_MTU - UDP_HEADER_LEN - min_ip_header_len).min(max_plpmtu);

        Self {
            state: State::Disabled,
            plpmtu: BASE_PLPMTU,
            probed_size: initial_probed_size,
            max_plpmtu,
            max_probe_size: max_plpmtu,
            probe_count: 0,
            pmtu_raise_timer: VirtualTimer::default(),
        }
    }

    /// Enable path MTU probing
    pub fn enable(&mut self) {
        debug_assert_eq!(self.state, State::Disabled);

        // TODO: Look up current MTU in a cache. If there is a cache hit
        //       move directly to SearchComplete and arm the PMTU raise timer.
        //       Otherwise, start searching for a larger PMTU immediately
        self.request_new_search(None);
    }

    /// Returns all timers for the component
    pub fn timers(&self) -> impl Iterator<Item = Timestamp> {
        self.pmtu_raise_timer.iter()
    }

    /// Called when the connection timer expires
    pub fn on_timeout(&mut self, now: Timestamp) {
        if self.pmtu_raise_timer.poll_expiration(now).is_ready() {
            self.request_new_search(None);
        }
    }

    //= https://tools.ietf.org/rfc/rfc8899.txt#4.2
    //# When
    //# supported, this mechanism MAY also be used by DPLPMTUD to acknowledge
    //# reception of a probe packet.
    /// This method gets called when a packet delivery got acknowledged
    pub fn on_packet_ack<A: ack::Set, CC: CongestionController>(
        &mut self,
        ack_set: &A,
        congestion_controller: &mut CC,
    ) {
        if let State::Searching(packet_number, transmit_time) = self.state {
            if ack_set.contains(packet_number) {
                self.plpmtu = self.probed_size;
                // A new MTU has been confirmed, notify the congestion controller
                congestion_controller.on_mtu_update(self.plpmtu);

                self.update_probed_size();

                //= https://tools.ietf.org/rfc/rfc8899.txt#8
                //# To avoid excessive load, the interval between individual probe
                //# packets MUST be at least one RTT, and the interval between rounds of
                //# probing is determined by the PMTU_RAISE_TIMER.

                // Subsequent probe packets are sent based on the round trip transmission and
                // acknowledgement/loss of a packet, so the interval will be at least 1 RTT.
                self.request_new_search(Some(transmit_time));
            }
        }
    }

    //= https://tools.ietf.org/rfc/rfc8899.txt#3
    //# The PL is REQUIRED to be
    //# robust in the case where probe packets are lost due to other
    //# reasons (including link transmission error, congestion).
    /// This method gets called when a packet loss is reported
    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        if let State::Searching(packet_number, _) = self.state {
            if ack_set.contains(packet_number) {
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
        }
    }

    /// Queries the component for any outgoing frames that need to get sent
    ///
    /// This method assumes that no other data (other than the packet header) has been written
    /// to the supplied `WriteContext`. This necessitates the caller ensuring the probe packet
    /// written by this method to be in its own connection transmission.
    pub fn on_transmit<W: WriteContext>(&mut self, context: &mut W) -> Result<(), OnTransmitError> {
        if self.state != State::SearchRequested || !context.transmission_mode().is_mtu_probing() {
            //= https://tools.ietf.org/rfc/rfc8899.txt#5.2
            //# When used with an acknowledged PL (e.g., SCTP), DPLPMTUD SHOULD NOT continue to
            //# generate PLPMTU probes in this state.
            return Ok(());
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
            return Ok(());
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#14.4
        //# Endpoints could limit the content of PMTU probes to PING and PADDING
        //# frames, since packets that are larger than the current maximum
        //# datagram size are more likely to be dropped by the network.

        //= https://tools.ietf.org/rfc/rfc8899.txt#3
        //# Probe loss recovery: It is RECOMMENDED to use probe packets that
        //# do not carry any user data that would require retransmission if
        //# lost.

        //= https://tools.ietf.org/rfc/rfc8899.txt#4.1
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

        Ok(())
    }

    /// Gets the currently validated maximum transmission unit
    pub fn mtu(&self) -> usize {
        self.plpmtu as usize
    }

    /// Sets `probed_size` to the next MTU size to probe for based on a binary search
    fn update_probed_size(&mut self) {
        //= https://tools.ietf.org/rfc/rfc8899.txt#5.3.2
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
                self.arm_pmtu_raise_timer(last_probe_time);
            }
        }
    }

    /// Arm the PMTU Raise Timer if there is still room to increase the
    /// MTU before hitting the max plpmtu
    fn arm_pmtu_raise_timer(&mut self, now: Timestamp) {
        // Reset the max_probe_size to the max_plpmtu to allow for larger probe sizes
        self.max_probe_size = self.max_plpmtu;
        self.update_probed_size();

        if self.probed_size - self.plpmtu >= PROBE_THRESHOLD {
            // There is still some room to try a larger MTU again,
            // so arm the pmtu raise timer
            self.pmtu_raise_timer.set(now + PMTU_RAISE_TIMER_DURATION);
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

#[cfg(test)]
mod test {
    use super::*;
    use crate::contexts::testing::{MockWriteContext, OutgoingFrameBuffer};
    use s2n_quic_core::{
        endpoint,
        frame::Frame,
        packet::number::{PacketNumberRange, PacketNumberSpace},
        recovery::congestion_controller::testing::mock::CongestionController,
        varint::VarInt,
    };
    use s2n_quic_platform::time::now;
    use std::net::SocketAddr;

    /// Creates a new mtu::Controller with an IPv4 address and the given `max_plpmtu`
    fn new_controller(max_plpmtu: u16) -> Controller {
        let addr: SocketAddr = "127.0.0.1:443".parse().unwrap();
        Controller::new(max_plpmtu, addr.into())
    }

    /// Creates an application space packet number with the given value
    pub fn pn(nr: usize) -> PacketNumber {
        PacketNumberSpace::ApplicationData.new_packet_number(VarInt::new(nr as u64).unwrap())
    }

    #[test]
    fn base_plpmtu_is_1200() {
        //= https://tools.ietf.org/rfc/rfc8899.txt#5.1.2
        //= type=test
        //# When using
        //# IPv4, there is no currently equivalent size specified, and a
        //# default BASE_PLPMTU of 1200 bytes is RECOMMENDED.
        assert_eq!(BASE_PLPMTU, 1200);
    }

    #[test]
    #[should_panic]
    fn new_max_plpmtu_too_small() {
        new_controller(BASE_PLPMTU - 1);
    }

    #[test]
    fn new_max_plpmtu_smaller_than_common_mtu() {
        let mut controller = new_controller(BASE_PLPMTU + 1);
        assert_eq!(BASE_PLPMTU + 1, controller.probed_size);

        controller.enable();
        assert_eq!(State::SearchComplete, controller.state);
    }

    #[test]
    fn new_ipv4() {
        let addr: SocketAddr = "127.0.0.1:443".parse().unwrap();
        let controller = Controller::new(1500, addr.into());
        assert_eq!(1500, controller.max_plpmtu);
        assert_eq!(1500, controller.max_probe_size);
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
        let controller = Controller::new(2000, addr.into());
        assert_eq!(2000, controller.max_plpmtu);
        assert_eq!(2000, controller.max_probe_size);
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
    #[should_panic]
    fn enable_already_enabled() {
        let mut controller = new_controller(1500);
        controller.enable();
        controller.enable();
    }

    #[test]
    fn enable() {
        let mut controller = new_controller(1500);
        controller.enable();
        assert_eq!(State::SearchRequested, controller.state);
    }

    //= https://tools.ietf.org/rfc/rfc8899.txt#4.2
    //= type=test
    //# When
    //# supported, this mechanism MAY also be used by DPLPMTUD to acknowledge
    //# reception of a probe packet.
    #[test]
    fn on_packet_ack_within_threshold() {
        let max_plpmtu = 1472 + PROBE_THRESHOLD * 2;
        let mut controller = new_controller(max_plpmtu);
        let pn = pn(1);
        let mut cc = CongestionController::default();
        let now = now();
        controller.state = State::Searching(pn, now);
        controller.probed_size = BASE_PLPMTU;
        controller.max_probe_size = BASE_PLPMTU + PROBE_THRESHOLD * 2 - 1;

        controller.on_packet_ack(&PacketNumberRange::new(pn, pn), &mut cc);

        assert_eq!(
            BASE_PLPMTU + (max_plpmtu - BASE_PLPMTU) / 2,
            controller.probed_size
        );
        assert_eq!(1, cc.on_mtu_update);
        assert_eq!(State::SearchComplete, controller.state);
        assert!(controller.pmtu_raise_timer.is_armed());
        assert_eq!(
            Some(now + PMTU_RAISE_TIMER_DURATION),
            controller.timers().next()
        );

        // Enough time passes that its time to try raising the PMTU again
        let now = now + PMTU_RAISE_TIMER_DURATION;
        controller.on_timeout(now);

        assert_eq!(State::SearchRequested, controller.state);
        assert_eq!(
            BASE_PLPMTU + (max_plpmtu - BASE_PLPMTU) / 2,
            controller.probed_size
        );
    }

    #[test]
    fn on_packet_ack_within_threshold_of_max_plpmtu() {
        let max_plpmtu = 1472 + (PROBE_THRESHOLD * 2 - 1);
        let mut controller = new_controller(max_plpmtu);
        let pn = pn(1);
        let mut cc = CongestionController::default();
        let now = now();
        controller.state = State::Searching(pn, now);

        controller.on_packet_ack(&PacketNumberRange::new(pn, pn), &mut cc);

        assert_eq!(1472 + (max_plpmtu - 1472) / 2, controller.probed_size);
        assert_eq!(1, cc.on_mtu_update);
        assert_eq!(State::SearchComplete, controller.state);
        assert!(!controller.pmtu_raise_timer.is_armed());
    }

    //= https://tools.ietf.org/rfc/rfc8899.txt#5.3.2
    //= type=test
    //# Implementations SHOULD select the set of probe packet sizes to
    //# maximize the gain in PLPMTU from each search step.

    //= https://tools.ietf.org/rfc/rfc8899.txt#8
    //= type=test
    //# To avoid excessive load, the interval between individual probe
    //# packets MUST be at least one RTT, and the interval between rounds of
    //# probing is determined by the PMTU_RAISE_TIMER.
    #[test]
    fn on_packet_ack_search_requested() {
        let max_plpmtu = 1472 + (PROBE_THRESHOLD * 2);
        let mut controller = new_controller(max_plpmtu);
        let pn = pn(1);
        let mut cc = CongestionController::default();
        let now = now();
        controller.state = State::Searching(pn, now);

        controller.on_packet_ack(&PacketNumberRange::new(pn, pn), &mut cc);

        assert_eq!(1472 + (max_plpmtu - 1472) / 2, controller.probed_size);
        assert_eq!(1, cc.on_mtu_update);
        assert_eq!(State::SearchRequested, controller.state);
        assert!(!controller.pmtu_raise_timer.is_armed());
    }

    //= https://tools.ietf.org/rfc/rfc8899.txt#3
    //= type=test
    //# The PL is REQUIRED to be
    //# robust in the case where probe packets are lost due to other
    //# reasons (including link transmission error, congestion).
    #[test]
    fn on_packet_loss() {
        let max_plpmtu = 1500;
        let mut controller = new_controller(max_plpmtu);
        let pn = pn(1);
        let now = now();
        controller.state = State::Searching(pn, now);
        let probed_size = controller.probed_size;

        controller.on_packet_loss(&PacketNumberRange::new(pn, pn));

        assert_eq!(max_plpmtu, controller.max_probe_size);
        assert_eq!(probed_size, controller.probed_size);
        assert_eq!(State::SearchRequested, controller.state);
    }

    #[test]
    fn on_packet_loss_max_probes() {
        let max_plpmtu = 1500;
        let mut controller = new_controller(max_plpmtu);
        let pn = pn(1);
        let now = now();
        controller.state = State::Searching(pn, now);
        controller.probe_count = MAX_PROBES;
        assert_eq!(max_plpmtu, controller.max_probe_size);

        controller.on_packet_loss(&PacketNumberRange::new(pn, pn));

        assert_eq!(1472, controller.max_probe_size);
        assert_eq!(
            BASE_PLPMTU + (1472 - BASE_PLPMTU) / 2,
            controller.probed_size
        );
        assert_eq!(State::SearchRequested, controller.state);
    }

    //= https://tools.ietf.org/rfc/rfc8899.txt#5.2
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

        assert!(controller.on_transmit(&mut write_context).is_ok());
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

        assert!(controller.on_transmit(&mut write_context).is_ok());
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

        assert!(controller.on_transmit(&mut write_context).is_ok());
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

        assert!(controller.on_transmit(&mut write_context).is_ok());
        assert!(frame_buffer.is_empty());
        assert_eq!(State::SearchComplete, controller.state);
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#14.4
    //= type=test
    //# Endpoints could limit the content of PMTU probes to PING and PADDING
    //# frames, since packets that are larger than the current maximum
    //# datagram size are more likely to be dropped by the network.

    //= https://tools.ietf.org/rfc/rfc8899.txt#3
    //= type=test
    //# Probe loss recovery: It is RECOMMENDED to use probe packets that
    //# do not carry any user data that would require retransmission if
    //# lost.

    //= https://tools.ietf.org/rfc/rfc8899.txt#4.1
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

        assert!(controller.on_transmit(&mut write_context).is_ok());
        assert_eq!(0, write_context.remaining_capacity());
        assert_eq!(
            Frame::Ping { 0: frame::Ping },
            write_context.frame_buffer.pop_front().unwrap().as_frame()
        );
        assert_eq!(
            Frame::Padding {
                0: frame::Padding {
                    length: controller.probed_size as usize - 1
                }
            },
            write_context.frame_buffer.pop_front().unwrap().as_frame()
        );
        assert_eq!(State::Searching(packet_number, now), controller.state);
    }
}
