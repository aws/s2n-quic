// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection::{self, SharedConnectionState},
    endpoint, path,
    path::Path,
    recovery::congestion_controller,
    transmission,
};
use core::time::Duration;
use s2n_codec::{Encoder, EncoderBuffer};
use s2n_quic_core::{
    frame::ack_elicitation::AckElicitable,
    inet::{ExplicitCongestionNotification, SocketAddress},
    io::tx,
    packet::{encoding::PacketEncodingError, number::PacketNumberSpace},
    time::Timestamp,
};

#[derive(Debug)]
pub struct ConnectionTransmissionContext<'a, Config: endpoint::Config> {
    pub quic_version: u32,
    pub timestamp: Timestamp,
    pub path_id: path::Id,
    pub path_manager: &'a mut path::Manager<Config::CongestionControllerEndpoint>,
    pub local_id_registry: &'a mut connection::LocalIdRegistry,
    pub source_connection_id: &'a connection::LocalId,
    pub outcome: &'a mut transmission::Outcome,
    pub ecn: ExplicitCongestionNotification,
    pub min_packet_len: Option<usize>,
    pub transmission_mode: transmission::Mode,
}

impl<'a, Config: endpoint::Config> ConnectionTransmissionContext<'a, Config> {
    pub fn path(
        &self,
    ) -> &Path<<Config::CongestionControllerEndpoint as congestion_controller::Endpoint>::CongestionController>
    {
        &self.path_manager[self.path_id]
    }

    pub fn path_mut(
        &mut self
    ) -> &mut Path<<Config::CongestionControllerEndpoint as congestion_controller::Endpoint>::CongestionController>
    {
        &mut self.path_manager[self.path_id]
    }
}

pub struct ConnectionTransmission<'a, Config: endpoint::Config> {
    pub context: ConnectionTransmissionContext<'a, Config>,
    pub shared_state: &'a mut SharedConnectionState<Config>,
}

impl<'a, Config: endpoint::Config> tx::Message for ConnectionTransmission<'a, Config> {
    fn remote_address(&mut self) -> SocketAddress {
        self.context.path().peer_socket_address
    }

    fn ecn(&mut self) -> ExplicitCongestionNotification {
        self.context.ecn
    }

    fn delay(&mut self) -> Duration {
        // TODO return delay from pacer
        Default::default()
    }

    fn ipv6_flow_label(&mut self) -> u32 {
        // TODO compute flow label from connection id
        0
    }

    fn write_payload(&mut self, buffer: &mut [u8]) -> usize {
        let shared_state = &mut self.shared_state;
        let space_manager = &mut shared_state.space_manager;

        let mtu = self
            .context
            .path()
            .clamp_mtu(buffer.len(), self.context.transmission_mode);
        debug_assert_ne!(
            mtu, 0,
            "the amplification limit should be checked before trying to transmit"
        );

        let buffer = &mut buffer[..mtu];

        let encoder = EncoderBuffer::new(buffer);
        let initial_capacity = encoder.capacity();

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7
        //# An endpoint MUST NOT send a packet if it would cause bytes_in_flight
        //# (see Appendix B.2) to be larger than the congestion window, unless
        //# the packet is sent on a PTO timer expiration (see Section 6.2) or
        //# when entering recovery (see Section 7.3.2).

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.4
        //# In addition to sending data in the packet number space for which the
        //# timer expired, the sender SHOULD send ack-eliciting packets from
        //# other packet number spaces with in-flight data, coalescing packets if
        //# possible.
        let transmission_constraint =
            if space_manager.requires_probe() && self.context.transmission_mode.is_normal() {
                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.4
                //# When the PTO timer expires, an ack-eliciting packet MUST be sent.  An
                //# endpoint SHOULD include new data in this packet.  Previously sent
                //# data MAY be sent if no new data can be sent.

                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.5
                //# Probe packets MUST NOT be blocked by the congestion controller.
                self.context.transmission_mode = transmission::Mode::LossRecoveryProbing;
                transmission::Constraint::None
            } else {
                self.context.path().transmission_constraint()
            };

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#14.1
        //# A client MUST expand the payload of all UDP datagrams carrying
        //# Initial packets to at least the smallest allowed maximum datagram
        //# size of 1200 bytes by adding PADDING frames to the Initial packet or
        //# by coalescing the Initial packet; see Section 12.2.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#14.1
        //# Similarly, a
        //# server MUST expand the payload of all UDP datagrams carrying ack-
        //# eliciting Initial packets to at least the smallest allowed maximum
        //# datagram size of 1200 bytes.

        // If the transmission contains an Initial packet, it must be padded. However, we should
        // add padding only if necessary, after packets from all packet number spaces have been
        // coalesced. Therefore, after confirming there will be an Initial packet, we first check
        // if there will be an ApplicationData packet, since those packets come at the end of the
        // datagram. If there is no ApplicationData packet, the Handshake packet will come at the
        // end, so we check that next. Finally, if there is no ApplicationData or Handshake packet
        // to transmit, the Initial packet itself will be padded.
        let mut pn_space_to_pad = {
            if !has_transmission(space_manager.initial(), transmission_constraint) {
                // There is no Initial packet, so no padding is needed
                None
            } else if has_transmission(space_manager.application(), transmission_constraint) {
                Some(PacketNumberSpace::ApplicationData)
            } else if has_transmission(space_manager.handshake(), transmission_constraint) {
                Some(PacketNumberSpace::Handshake)
            } else {
                //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#4.9
                //# These packets MAY also include PADDING frames.
                Some(PacketNumberSpace::Initial)
            }
        };

        //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#4
        //# When packets of different types need to be sent,
        //# endpoints SHOULD use coalesced packets to send them in the same UDP
        //# datagram.
        // here we query all of the spaces to try and fill the current datagram

        let encoder = if let Some((space, handshake_status)) = space_manager.initial_mut() {
            self.context.min_packet_len = pn_space_to_pad
                .filter(|pn_space| pn_space.is_initial())
                .map(|_| encoder.capacity());

            match space.on_transmit(
                &mut self.context,
                transmission_constraint,
                handshake_status,
                encoder,
            ) {
                Ok((outcome, encoder)) => {
                    *self.context.outcome += outcome;

                    if Config::ENDPOINT_TYPE.is_server()
                        && !outcome.ack_elicitation().is_ack_eliciting()
                    {
                        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#14.1
                        //# Similarly, a
                        //# server MUST expand the payload of all UDP datagrams carrying ack-
                        //# eliciting Initial packets to at least the smallest allowed maximum
                        //# datagram size of 1200 bytes.

                        // The Initial packet was not ack eliciting so there is no need to pad
                        pn_space_to_pad = None;
                    }
                    encoder
                }
                Err(PacketEncodingError::PacketNumberTruncationError(encoder)) => {
                    // TODO handle this
                    encoder
                }
                Err(PacketEncodingError::InsufficientSpace(encoder)) => {
                    // move to the next packet space
                    encoder
                }
                Err(PacketEncodingError::EmptyPayload(encoder)) => {
                    // move to the next packet space
                    encoder
                }
                Err(PacketEncodingError::AeadLimitReached(encoder)) => {
                    // move to the next packet space
                    encoder
                }
            }
        } else {
            encoder
        };

        let encoder = if let Some((space, handshake_status)) = space_manager.handshake_mut() {
            self.context.min_packet_len = pn_space_to_pad
                .filter(|pn_space| pn_space.is_handshake())
                .map(|_| encoder.capacity());

            let encoder = match space.on_transmit(
                &mut self.context,
                transmission_constraint,
                handshake_status,
                encoder,
            ) {
                Ok((outcome, encoder)) => {
                    *self.context.outcome += outcome;

                    //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#4.9.1
                    //# a client MUST discard Initial keys when it first sends a
                    //# Handshake packet

                    if Config::ENDPOINT_TYPE.is_client() {
                        space_manager.discard_initial(self.context.path_mut());
                    }

                    encoder
                }
                Err(PacketEncodingError::PacketNumberTruncationError(encoder)) => {
                    // TODO handle this
                    encoder
                }
                Err(PacketEncodingError::InsufficientSpace(encoder)) => {
                    // move to the next packet space
                    encoder
                }
                Err(PacketEncodingError::EmptyPayload(encoder)) => {
                    // move to the next packet space
                    encoder
                }
                Err(PacketEncodingError::AeadLimitReached(encoder)) => {
                    // move to the next packet space
                    encoder
                }
            };

            //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#4.9.2
            //# An endpoint MUST discard its handshake keys when the TLS handshake is
            //# confirmed (Section 4.1.2).
            if space_manager.is_handshake_confirmed() {
                space_manager.discard_handshake(self.context.path_mut());
            }

            encoder
        } else {
            encoder
        };

        //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#4.9
        //# Though an endpoint might retain older keys, new data MUST be sent at
        //# the highest currently-available encryption level.

        // This requirement is automatically support with s2n-quic's implementation. Each space
        // acts mostly independent from another and will buffer its own CRYPTO and ACK state. Other
        // frames are only allowed in the ApplicationData space, which will always be the highest
        // current-available encryption level.

        let encoder = if let Some((space, handshake_status)) = space_manager.application_mut() {
            self.context.min_packet_len = pn_space_to_pad
                .filter(|pn_space| pn_space.is_application_data())
                .map(|_| encoder.capacity());

            let path = &self.context.path_manager[self.context.path_id];
            let validation_frames_pending =
                path.is_challenge_pending() || path.is_response_pending();
            if !path.is_validated() && validation_frames_pending {
                self.context.min_packet_len = Some(encoder.capacity());
            }

            match space.on_transmit(
                &mut self.context,
                transmission_constraint,
                handshake_status,
                encoder,
            ) {
                Ok((outcome, encoder)) => {
                    *self.context.outcome += outcome;
                    encoder
                }
                Err(PacketEncodingError::PacketNumberTruncationError(encoder)) => {
                    // TODO handle this
                    encoder
                }
                Err(PacketEncodingError::InsufficientSpace(encoder)) => {
                    // move to the next packet space
                    encoder
                }
                Err(PacketEncodingError::EmptyPayload(encoder)) => {
                    // move to the next packet space
                    encoder
                }
                Err(PacketEncodingError::AeadLimitReached(encoder)) => {
                    // move to the next packet space
                    encoder
                }
            }
        } else {
            encoder
        };

        let datagram_len = initial_capacity - encoder.capacity();
        self.context.path_mut().on_bytes_transmitted(datagram_len);

        datagram_len
    }
}

fn has_transmission<P: transmission::interest::Provider>(
    transmission_interest_provider: Option<&P>,
    transmission_constraint: transmission::Constraint,
) -> bool {
    transmission_interest_provider.map_or(false, |provider| {
        provider
            .transmission_interest()
            .can_transmit(transmission_constraint)
    })
}
