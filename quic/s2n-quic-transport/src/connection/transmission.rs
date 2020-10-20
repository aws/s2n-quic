use crate::{
    connection::{self, SharedConnectionState},
    contexts::ConnectionContext,
};
use core::time::Duration;
use s2n_codec::{Encoder, EncoderBuffer};
use s2n_quic_core::{
    endpoint::EndpointType,
    frame::ack_elicitation::AckElicitation,
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
        if mtu == 0 {
            return 0;
        }
        let buffer = &mut buffer[..mtu];

        let encoder = EncoderBuffer::new(buffer);
        let initial_capacity = encoder.capacity();

        let encoder = if let Some(space) = space_manager.initial_mut() {
            match space.on_transmit(&mut self.context, encoder) {
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

        let encoder = if let Some(space) = space_manager.handshake_mut() {
            let encoder = match space.on_transmit(&mut self.context, encoder) {
                Ok(encoder) => {
                    //= https://tools.ietf.org/id/draft-ietf-quic-tls-27.txt#4.10.1
                    //# A client MUST discard Initial keys when it first sends a Handshake packet

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

            //= https://tools.ietf.org/id/draft-ietf-quic-tls-29#4.11.2
            //# An endpoint MUST discard its handshake keys when the TLS handshake is
            //# confirmed (Section 4.1.2).
            if let Some(application_space) = space_manager.application() {
                if application_space.handshake_status.is_confirmed() {
                    space_manager.discard_handshake(self.context.path);
                }
            }

            encoder
        } else {
            encoder
        };

        let encoder = if let Some(space) = space_manager.application_mut() {
            match space.on_transmit(&mut self.context, encoder) {
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

#[derive(Clone, Copy, Debug, Default)]
pub struct Outcome {
    pub ack_elicitation: AckElicitation,
    pub is_congestion_controlled: bool,
    pub bytes_sent: usize,
}
