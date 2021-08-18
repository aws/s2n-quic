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
    Testing,
    Unknown,
    Failed,
    Capable,
}

#[derive(Clone, Debug)]
pub struct Controller {
    state: State,
}

impl Controller {
    /// Construct a new mtu::Controller with the given `max_mtu` and `peer_socket_address`
    ///
    /// The UDP header length and IP header length will be subtracted from `max_mtu` to
    /// determine the max_udp_payload used for limiting the payload length of probe packets.
    pub fn new() -> Self {
        Self {
            state: State::Unknown,
        }
    }

    /// Enable path MTU probing
    pub fn enable(&mut self) {
        if self.state != State::Unknown {
            return;
        }

        self.state = State::Testing
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
    pub fn on_packet_ack<CC: CongestionController>(
        &mut self,
        packet_number: PacketNumber,
        sent_bytes: u16,
        congestion_controller: &mut CC,
    ) {
    }

    pub fn on_ack_frame<A: frame::ack::AckRanges>(&mut self, frame: frame::Ack<A>) {

        // If an ACK frame newly acknowledges a packet that the endpoint sent with either the ECT(0) or ECT(1) codepoint set,
        // ECN validation fails if the corresponding ECN counts are not present in the ACK frame.
        // This check detects a network element that zeroes the ECN field or a peer that does not report ECN markings.
        // if sent_packet_info.ecn in (0,1) && frame.ecn_counts.is_none() => fail validation

        // ECN validation also fails if the sum of the increase in ECT(0) and ECN-CE counts is
        // less than the number of newly acknowledged packets that were originally sent with an ECT(0) marking
        // let increase = frame.ecn_counts.ect_0_count + frame.ecn_counts.ce_count - (original ect0 + ce count)
        // increase < sum newly_acked_packets.ecn = 0

        //Similarly, ECN validation fails if the sum of the increases to ECT(1) and ECN-CE counts is
        // less than the number of newly acknowledged packets sent with an ECT(1) marking.
        // These checks can detect remarking of ECN-CE markings by the network.
        // let increase = frame.ecn_counts.ect_1_count + frame.ecn_counts.ce_count - (original ect1 + ce count)
        // increase < sum newly_acked_packets.ecn = 1
    }

    //= https://tools.ietf.org/rfc/rfc8899.txt#3
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
}
