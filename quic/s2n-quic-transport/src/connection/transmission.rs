use crate::{
    connection::{self, SharedConnectionState},
    contexts::ConnectionContext,
};
use core::time::Duration;
use s2n_codec::{Encoder, EncoderBuffer};
use s2n_quic_core::{
    endpoint::EndpointType,
    inet::{ExplicitCongestionNotification, SocketAddress},
    io::tx,
    packet::encoding::PacketEncodingError,
    time::Timestamp,
};

#[derive(Clone, Copy, Debug)]
pub struct ConnectionTransmissionContext {
    pub quic_version: u32,
    pub source_connection_id: connection::ID,
    pub destination_connection_id: connection::ID,
    pub timestamp: Timestamp,
    pub local_endpoint_type: EndpointType,
    pub remote_address: SocketAddress,
    pub ecn: ExplicitCongestionNotification,
}

impl ConnectionContext for ConnectionTransmissionContext {
    fn local_endpoint_type(&self) -> EndpointType {
        self.local_endpoint_type
    }

    fn connection_id(&self) -> &connection::ID {
        &self.source_connection_id
    }
}

pub struct ConnectionTransmission<'a, ConnectionConfigType: connection::Config> {
    pub context: ConnectionTransmissionContext,
    pub shared_state: &'a mut SharedConnectionState<ConnectionConfigType>,
}

impl<'a, ConnectionConfigType: connection::Config> tx::Message
    for ConnectionTransmission<'a, ConnectionConfigType>
{
    fn remote_address(&mut self) -> SocketAddress {
        self.context.remote_address
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
        // TODO trim off based on path MTU
        // TODO trim off based on congestion controller

        let encoder = EncoderBuffer::new(buffer);
        let initial_capacity = encoder.capacity();

        let shared_state = &mut self.shared_state;
        let space_manager = &mut shared_state.space_manager;

        let encoder = if let Some(space) = space_manager.initial_mut() {
            match space.on_transmit(&self.context, encoder) {
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
            let encoder = match space.on_transmit(&self.context, encoder) {
                Ok(encoder) => {
                    //= https://tools.ietf.org/id/draft-ietf-quic-tls-27.txt#4.10.1
                    //# A client MUST discard Initial keys when it first sends a Handshake packet

                    if ConnectionConfigType::ENDPOINT_TYPE.is_client() {
                        space_manager.discard_initial();
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
                    space_manager.discard_handshake();
                }
            }

            encoder
        } else {
            encoder
        };

        let encoder = if let Some(space) = space_manager.application_mut() {
            match space.on_transmit(&self.context, encoder) {
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
