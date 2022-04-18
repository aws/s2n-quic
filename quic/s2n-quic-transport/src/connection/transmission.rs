// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection, endpoint, path, path::Path, space::PacketSpaceManager, transmission,
    transmission::interest::Provider,
};
use core::time::Duration;
use s2n_codec::{Encoder, EncoderBuffer};
use s2n_quic_core::{
    event::{self, ConnectionPublisher as _},
    frame::ack_elicitation::AckElicitable,
    inet::ExplicitCongestionNotification,
    io::tx,
    packet::{encoding::PacketEncodingError, number::PacketNumberSpace},
    time::Timestamp,
};

#[derive(Debug)]
pub struct ConnectionTransmissionContext<'a, 'sub, Config: endpoint::Config> {
    pub quic_version: u32,
    pub timestamp: Timestamp,
    pub path_id: path::Id,
    pub path_manager: &'a mut path::Manager<Config>,
    pub local_id_registry: &'a mut connection::LocalIdRegistry,
    pub outcome: &'a mut transmission::Outcome,
    pub ecn: ExplicitCongestionNotification,
    pub min_packet_len: Option<usize>,
    pub transmission_mode: transmission::Mode,
    pub publisher: &'a mut event::ConnectionPublisherSubscriber<'sub, Config::EventSubscriber>,
    pub packet_interceptor: &'a mut Config::PacketInterceptor,
}

impl<'a, 'sub, Config: endpoint::Config> ConnectionTransmissionContext<'a, 'sub, Config> {
    pub fn path(&self) -> &Path<Config> {
        &self.path_manager[self.path_id]
    }

    pub fn path_mut(&mut self) -> &mut Path<Config> {
        &mut self.path_manager[self.path_id]
    }
}

pub struct ConnectionTransmission<'a, 'sub, Config: endpoint::Config> {
    pub context: ConnectionTransmissionContext<'a, 'sub, Config>,
    pub space_manager: &'a mut PacketSpaceManager<Config>,
}

impl<'a, 'sub, Config: endpoint::Config> tx::Message for ConnectionTransmission<'a, 'sub, Config> {
    type Handle = Config::PathHandle;

    #[inline]
    fn path_handle(&self) -> &Self::Handle {
        &self.context.path().handle
    }

    #[inline]
    fn ecn(&mut self) -> ExplicitCongestionNotification {
        self.context.ecn
    }

    #[inline]
    fn delay(&mut self) -> Duration {
        // TODO return delay from pacer
        Default::default()
    }

    #[inline]
    fn ipv6_flow_label(&mut self) -> u32 {
        // TODO compute flow label from connection id
        0
    }

    #[inline]
    fn can_gso(&self) -> bool {
        // If a packet can be GSO'd it means it's limited to the previously written packet
        // size. This becomes a problem for MTU probes where they will likely exceed that amount.
        // As such, if we're probing we want to let the IO layer know to not GSO the current
        // packet.
        !self.context.transmission_mode.is_mtu_probing()
    }

    fn write_payload(&mut self, buffer: &mut [u8], gso_offset: usize) -> usize {
        let space_manager = &mut self.space_manager;

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

        //= https://www.rfc-editor.org/rfc/rfc9002#section-7
        //# An endpoint MUST NOT send a packet if it would cause bytes_in_flight
        //# (see Appendix B.2) to be larger than the congestion window, unless
        //# the packet is sent on a PTO timer expiration (see Section 6.2) or
        //# when entering recovery (see Section 7.3.2).

        //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.4
        //# In addition to sending data in the packet number space for which the
        //# timer expired, the sender SHOULD send ack-eliciting packets from
        //# other packet number spaces with in-flight data, coalescing packets if
        //# possible.
        let transmission_constraint =
            if space_manager.requires_probe() && self.context.transmission_mode.is_normal() {
                //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.4
                //# When a PTO timer expires, a sender MUST send at least one ack-
                //# eliciting packet in the packet number space as a probe.

                //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.4
                //# An endpoint SHOULD include new data in packets that are sent on PTO
                //# expiration.  Previously sent data MAY be sent if no new data can be
                //# sent.

                //= https://www.rfc-editor.org/rfc/rfc9002#section-7.5
                //# Probe packets MUST NOT be blocked by the congestion controller.
                self.context.transmission_mode = transmission::Mode::LossRecoveryProbing;
                transmission::Constraint::None
            } else {
                self.context.path().transmission_constraint()
            };

        //= https://www.rfc-editor.org/rfc/rfc9000#section-14.1
        //# A client MUST expand the payload of all UDP datagrams carrying
        //# Initial packets to at least the smallest allowed maximum datagram
        //# size of 1200 bytes by adding PADDING frames to the Initial packet or
        //# by coalescing the Initial packet; see Section 12.2.

        //= https://www.rfc-editor.org/rfc/rfc9000#section-14.1
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
                //= https://www.rfc-editor.org/rfc/rfc9001#section-4.9
                //# These packets MAY also include PADDING frames.
                Some(PacketNumberSpace::Initial)
            }
        };

        //= https://www.rfc-editor.org/rfc/rfc9001#section-4
        //# When packets of different types need to be sent,
        //# endpoints SHOULD use coalesced packets to send them in the same UDP
        //# datagram.
        // here we query all of the spaces to try and fill the current datagram

        let is_mtu_probing = self.context.transmission_mode.is_mtu_probing();

        let encoder = if let Some((space, handshake_status)) = space_manager
            .initial_mut()
            // MTU probes are only sent in the Application Space
            .filter(|_| !is_mtu_probing)
        {
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
                    if Config::ENDPOINT_TYPE.is_server()
                        && !outcome.ack_elicitation().is_ack_eliciting()
                    {
                        //= https://www.rfc-editor.org/rfc/rfc9000#section-14.1
                        //# Similarly, a
                        //# server MUST expand the payload of all UDP datagrams carrying ack-
                        //# eliciting Initial packets to at least the smallest allowed maximum
                        //# datagram size of 1200 bytes.

                        // The Initial packet was not ack eliciting so there is no need to pad
                        pn_space_to_pad = None;
                    }
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

        let encoder = if let Some((space, handshake_status)) = space_manager
            .handshake_mut()
            // MTU probes are only sent in the Application Space
            .filter(|_| !is_mtu_probing)
        {
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
                    //= https://www.rfc-editor.org/rfc/rfc9001#section-4.9.1
                    //# a client MUST discard Initial keys when it first sends a
                    //# Handshake packet

                    if Config::ENDPOINT_TYPE.is_client() {
                        let path = &mut self.context.path_manager[self.context.path_id];
                        space_manager.discard_initial(
                            path,
                            self.context.path_id,
                            self.context.publisher,
                        );
                    }

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
            };

            //= https://www.rfc-editor.org/rfc/rfc9001#section-4.9.2
            //# An endpoint MUST discard its handshake keys when the TLS handshake is
            //# confirmed (Section 4.1.2).
            if space_manager.is_handshake_confirmed() {
                let path = &mut self.context.path_manager[self.context.path_id];
                space_manager.discard_handshake(path, self.context.path_id, self.context.publisher);
            }

            encoder
        } else {
            encoder
        };

        //= https://www.rfc-editor.org/rfc/rfc9001#section-4.9
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

            // Pad the packet when sending path validation frames so that MTU is also validated.
            let path = &self.context.path_manager[self.context.path_id];

            //= https://www.rfc-editor.org/rfc/rfc9000#section-8.2.1
            //# An endpoint MUST expand datagrams that contain a PATH_CHALLENGE frame
            //# to at least the smallest allowed maximum datagram size of 1200 bytes.
            //
            //= https://www.rfc-editor.org/rfc/rfc9000#section-8.2.2
            //# An endpoint MUST expand datagrams that contain a PATH_RESPONSE frame
            //# to at least the smallest allowed maximum datagram size of 1200 bytes.
            // Pad the packet when sending path validation frames so that MTU is also validated.
            //
            // The path's transmission_interest indicates if a PATH_CHALLENGE or PATH_RESPONSE
            // frame is to be written.
            //
            // We need to check is_validated because it is possible to receive a PATH_CHALLENGE on
            // an active path for Off-Path Packet Forwarding prevention. However, we would only
            // like to pad when validating the MTU.
            if !path.is_validated() && path.has_transmission_interest() {
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

        if datagram_len > 0 {
            self.context
                .publisher
                .on_datagram_sent(event::builder::DatagramSent {
                    len: datagram_len as u16,
                    gso_offset,
                });
        }

        datagram_len
    }
}

fn has_transmission<P: transmission::interest::Provider>(
    transmission_interest_provider: Option<&P>,
    transmission_constraint: transmission::Constraint,
) -> bool {
    transmission_interest_provider.map_or(false, |provider| {
        provider.can_transmit(transmission_constraint)
    })
}
