use crate::{
    connection::{
        self, connection_id_mapper::ConnectionIdMapperRegistration, SharedConnectionState,
    },
    path, transmission,
};
use core::time::Duration;
use s2n_codec::{Encoder, EncoderBuffer};
use s2n_quic_core::{
    inet::{ExplicitCongestionNotification, SocketAddress},
    io::tx,
    packet::encoding::PacketEncodingError,
    time::Timestamp,
};

#[derive(Debug)]
pub struct ConnectionTransmissionContext<'a, Config: connection::Config> {
    pub quic_version: u32,
    pub timestamp: Timestamp,
    pub path_manager: &'a mut path::Manager<Config::CongestionController>,
    pub connection_id_mapper_registration: &'a mut ConnectionIdMapperRegistration,
    pub source_connection_id: &'a connection::LocalId,
    pub ecn: ExplicitCongestionNotification,
}

pub struct ConnectionTransmission<'a, Config: connection::Config> {
    pub context: ConnectionTransmissionContext<'a, Config>,
    pub shared_state: &'a mut SharedConnectionState<Config>,
}

impl<'a, Config: connection::Config> tx::Message for ConnectionTransmission<'a, Config> {
    fn remote_address(&mut self) -> SocketAddress {
        self.context
            .path_manager
            .active_path()
            .1
            .peer_socket_address
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
            .path_manager
            .active_path()
            .1
            .clamp_mtu(buffer.len());
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

            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.5
            //# Probe packets MUST NOT be blocked by the congestion controller.
            transmission::Constraint::None
        } else {
            self.context
                .path_manager
                .active_path()
                .1
                .transmission_constraint()
        };

        //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#4
        //# When packets of different types need to be sent,
        //# endpoints SHOULD use coalesced packets to send them in the same UDP
        //# datagram.
        // here we query all of the spaces to try and fill the current datagram

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
                        space_manager
                            .discard_initial(self.context.path_manager.active_path_mut().1);
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
                space_manager.discard_handshake(self.context.path_manager.active_path_mut().1);
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
