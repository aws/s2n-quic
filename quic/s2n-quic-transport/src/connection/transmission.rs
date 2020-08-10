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
    path::Path,
    time::Timestamp,
};

#[derive(Debug)]
pub struct ConnectionTransmissionContext<'a> {
    pub quic_version: u32,
    pub timestamp: Timestamp,
    pub local_endpoint_type: EndpointType,
    pub path: &'a mut Path,
    pub source_connection_id: &'a connection::Id,
    pub ecn: ExplicitCongestionNotification,
    pub send_limits: ConnectionSendLimits,
}

impl<'a> ConnectionContext for ConnectionTransmissionContext<'a> {
    fn local_endpoint_type(&self) -> EndpointType {
        self.local_endpoint_type
    }

    fn connection_id(&self) -> &connection::Id {
        &self.path.peer_connection_id
    }
}

pub struct ConnectionTransmission<'a, ConnectionConfigType: connection::Config> {
    pub context: ConnectionTransmissionContext<'a>,
    pub shared_state: &'a mut SharedConnectionState<ConnectionConfigType>,
}

impl<'a, ConnectionConfigType: connection::Config> tx::Message
    for ConnectionTransmission<'a, ConnectionConfigType>
{
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
        // TODO trim off based on congestion controller

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


#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        connection::{ConnectionImpl, InternalConnectionIdGenerator},
        endpoint::{ConnectionIdGenerator, EndpointConfig},
        space::PacketSpaceManager,
        wakeup_queue::WakeupQueue,
    };
    use s2n_quic_core::endpoint::EndpointType;

    /// A primitive connection ID generator for testing purposes.
    struct LocalConnectionIdGenerator {
        next_id: u64,
    }

    impl LocalConnectionIdGenerator {
        fn new() -> Self {
            Self { next_id: 1 }
        }
    }

    impl ConnectionIdGenerator for LocalConnectionIdGenerator {
        fn generate_connection_id(&mut self) -> ConnectionId {
            self.next_id += 1;
            let bytes = self.next_id.to_be_bytes();
            ConnectionId::try_from_bytes(&bytes).unwrap()
        }
    }

    struct TestConfig {}
    impl EndpointConfig for TestConfig {
        type ConnectionConfigType = TestConfig;
        type ConnectionIdGeneratorType = LocalConnectionIdGenerator;
        type ConnectionType = ConnectionImpl<Self::ConnectionConfigType>;
        type TLSEndpointType = RustlsServerEndpoint;

        fn create_connection_config(&mut self) -> Self::ConnectionConfigType {
            TestConfig {}
        }
    }

    #[test]
    fn test_byte_counting() {
        let connection_id = ConnectionId::try_from_bytes(&"0000000000000000".as_ref()).unwrap();
        let context = ConnectionTransmissionContext {
            quic_version: 0,
            destination_connection_id: connection_id,
            source_connection_id: connection_id,
            timestamp: unsafe { Timestamp::from_duration(Duration::from_millis(100)) },
            local_endpoint_type: EndpointType::Server,
            remote_address: Default::default(),
            ecn: Default::default(),
            send_limits: Default::default(),
        };

        let space_manager = PacketSpaceManager::<T> {
            session: None,
            initial: None,
            handshake: None,
            application: None,
            zero_rtt_crypto: None,
        };

        let generator = InternalConnectionIdGenerator::new();
        let internal_connection_id = generator.generate_id();
        let mut queue = WakeupQueue::new();
        let mut handle1 = queue.create_wakeup_handle(internal_connection_id);
        let state = SharedConnectionState::new(space_manager, handle1);
        let trans = ConnectionTransmission {
            context: context,
            shared_state: &mut state,
        };

    }
}
