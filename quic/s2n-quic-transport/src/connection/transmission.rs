use crate::{
    connection::{self, SharedConnectionState},
    contexts::ConnectionContext,
    transmission,
};
use core::time::Duration;
use s2n_codec::{Encoder, EncoderBuffer};
use s2n_quic_core::{
    endpoint::EndpointType,
    inet::{ExplicitCongestionNotification, SocketAddress},
    io::tx,
    packet::encoding::PacketEncodingError,
    path::Path,
    time::Timestamp,
};

#[derive(Debug)]
pub struct ConnectionTransmissionContext<'a, Config: connection::Config> {
    pub quic_version: u32,
    pub timestamp: Timestamp,
    pub path: &'a mut Path<Config::CongestionController>,
    pub source_connection_id: &'a connection::Id,
    pub ecn: ExplicitCongestionNotification,
}

impl<'a, Config: connection::Config> ConnectionContext
    for ConnectionTransmissionContext<'a, Config>
{
    fn local_endpoint_type(&self) -> EndpointType {
        Config::ENDPOINT_TYPE
    }

    fn connection_id(&self) -> &connection::Id {
        &self.path.peer_connection_id
    }
}

pub struct ConnectionTransmission<'a, Config: connection::Config> {
    pub context: ConnectionTransmissionContext<'a, Config>,
    pub shared_state: &'a mut SharedConnectionState<Config>,
}

impl<'a, Config: connection::Config> tx::Message for ConnectionTransmission<'a, Config> {
    fn remote_address(&mut self) -> SocketAddress {
        self.context.path.peer_socket_address
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

        let mtu = self.context.path.clamp_mtu(buffer.len());
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
        let transmission_constraint = if space_manager.requires_probe() {
            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.4
            //# When the PTO timer expires, an ack-eliciting packet MUST be sent.  An
            //# endpoint SHOULD include new data in this packet.  Previously sent
            //# data MAY be sent if no new data can be sent.
            transmission::Constraint::None
        } else {
            self.context.path.transmission_constraint()
        };

        let encoder = if let Some((space, handshake_status)) = space_manager.initial_mut() {
            match space.on_transmit(
                &mut self.context,
                transmission_constraint,
                handshake_status,
                encoder,
            ) {
                Ok(encoder) => encoder,
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
            }
        } else {
            encoder
        };

        let encoder = if let Some((space, handshake_status)) = space_manager.handshake_mut() {
            let encoder = match space.on_transmit(
                &mut self.context,
                transmission_constraint,
                handshake_status,
                encoder,
            ) {
                Ok(encoder) => {
                    //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#4.9.1
                    //# a client MUST discard Initial keys when it first sends a
                    //# Handshake packet

                    if Config::ENDPOINT_TYPE.is_client() {
                        space_manager.discard_initial(self.context.path);
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
            };

            //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#4.9.2
            //# An endpoint MUST discard its handshake keys when the TLS handshake is
            //# confirmed (Section 4.1.2).
            if space_manager.is_handshake_confirmed() {
                space_manager.discard_handshake(self.context.path);
            }

            encoder
        } else {
            encoder
        };

        let encoder = if let Some((space, handshake_status)) = space_manager.application_mut() {
            match space.on_transmit(
                &mut self.context,
                transmission_constraint,
                handshake_status,
                encoder,
            ) {
                Ok(encoder) => encoder,
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
            }
        } else {
            encoder
        };

        initial_capacity - encoder.capacity()
    }
}
