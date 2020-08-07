use crate::{
    connection::ConnectionConfig,
    space::{
        rx_packet_numbers::{AckManager, DEFAULT_ACK_RANGES_LIMIT, EARLY_ACK_SETTINGS},
        ApplicationSpace, HandshakeSpace, InitialSpace,
    },
    stream::AbstractStreamManager,
};
use bytes::Bytes;
use s2n_codec::{DecoderBuffer, DecoderValue};
use s2n_quic_core::{
    crypto::{
        tls::{TLSApplicationParameters, TLSContext},
        CryptoError, CryptoSuite,
    },
    packet::number::PacketNumberSpace,
    time::Timestamp,
    transport::parameters::ClientTransportParameters,
};

pub struct SessionContext<'a, ConnectionConfigType: ConnectionConfig> {
    pub now: Timestamp,
    pub connection_config: &'a ConnectionConfigType,
    pub initial: &'a mut Option<Box<InitialSpace<ConnectionConfigType::TLSSession>>>,
    pub handshake: &'a mut Option<Box<HandshakeSpace<ConnectionConfigType::TLSSession>>>,
    pub application: &'a mut Option<
        Box<ApplicationSpace<ConnectionConfigType::StreamType, ConnectionConfigType::TLSSession>>,
    >,
    pub zero_rtt_crypto:
        &'a mut Option<Box<<ConnectionConfigType::TLSSession as CryptoSuite>::ZeroRTTCrypto>>,
}

impl<'a, ConnectionConfigType: ConnectionConfig> TLSContext<ConnectionConfigType::TLSSession>
    for SessionContext<'a, ConnectionConfigType>
{
    fn on_handshake_keys(
        &mut self,
        keys: <ConnectionConfigType::TLSSession as CryptoSuite>::HandshakeCrypto,
    ) -> Result<(), CryptoError> {
        if let Some(initial) = self.initial.as_mut() {
            // TODO make sure the rx buffer is empty, otherwise it's a
            // transport violation
            initial.crypto_stream.finish();
        }

        let ack_manager = AckManager::new(
            PacketNumberSpace::Handshake,
            EARLY_ACK_SETTINGS,
            DEFAULT_ACK_RANGES_LIMIT,
        );

        *self.handshake = Some(Box::new(HandshakeSpace::new(keys, self.now, ack_manager)));

        Ok(())
    }

    fn on_zero_rtt_keys(
        &mut self,
        keys: <ConnectionConfigType::TLSSession as CryptoSuite>::ZeroRTTCrypto,
        _application_parameters: TLSApplicationParameters,
    ) -> Result<(), CryptoError> {
        *self.zero_rtt_crypto = Some(Box::new(keys));

        Ok(())
    }

    fn on_one_rtt_keys(
        &mut self,
        keys: <ConnectionConfigType::TLSSession as CryptoSuite>::OneRTTCrypto,
        application_parameters: TLSApplicationParameters,
    ) -> Result<(), CryptoError> {
        if let Some(handshake) = self.handshake.as_mut() {
            // TODO make sure the rx buffer is empty, otherwise it's a
            // transport violation
            handshake.crypto_stream.finish();
        }

        // Parse transport parameters
        // TODO: This assumes we are a server, and needs to be changed for the client
        let param_decoder = DecoderBuffer::new(application_parameters.transport_parameters);
        let (peer_parameters, remaining) = match ClientTransportParameters::decode(param_decoder) {
            Ok(parameters) => parameters,
            Err(_e) => {
                return Err(
                    CryptoError::MISSING_EXTENSION.with_reason("Invalid transport parameters")
                );
            }
        };

        if !remaining.is_empty() {
            return Err(
                CryptoError::MISSING_EXTENSION.with_reason("Invalid bytes in transport parameters")
            );
        }

        let peer_limits = peer_parameters.flow_control_limits();
        let local_flow_control_limits = *self.connection_config.local_flow_control_limits();

        let stream_manager = AbstractStreamManager::new(
            self.connection_config.connection_limits(),
            ConnectionConfigType::ENDPOINT_TYPE,
            local_flow_control_limits,
            peer_limits,
        );

        // TODO ack interval limit configurable
        let ack_interval_limit = DEFAULT_ACK_RANGES_LIMIT;
        let ack_settings = *self.connection_config.local_ack_settings();
        let ack_manager = AckManager::new(
            PacketNumberSpace::ApplicationData,
            ack_settings,
            ack_interval_limit,
        );

        *self.application = Some(Box::new(ApplicationSpace::new(
            keys,
            self.now,
            stream_manager,
            ack_manager,
        )));

        Ok(())
    }

    fn receive_initial(&mut self) -> Option<Bytes> {
        self.initial
            .as_mut()
            .map(Box::as_mut)?
            .crypto_stream
            .rx
            .pop()
            .map(|bytes| bytes.freeze())
    }

    fn receive_handshake(&mut self) -> Option<Bytes> {
        self.handshake
            .as_mut()
            .map(Box::as_mut)?
            .crypto_stream
            .rx
            .pop()
            .map(|bytes| bytes.freeze())
    }

    fn receive_application(&mut self) -> Option<Bytes> {
        // Application doesn't currently have a buffer
        None
    }

    fn can_send_initial(&self) -> bool {
        self.initial
            .as_ref()
            .map(|space| space.crypto_stream.can_send())
            .unwrap_or_default()
    }

    fn send_initial(&mut self, transmission: Bytes) {
        self.initial
            .as_mut()
            .expect("can_send_initial should be called before sending")
            .crypto_stream
            .tx
            .push(transmission);
    }

    fn can_send_handshake(&self) -> bool {
        self.handshake
            .as_ref()
            .map(|space| space.crypto_stream.can_send())
            .unwrap_or_default()
    }

    fn send_handshake(&mut self, transmission: Bytes) {
        self.handshake
            .as_mut()
            .expect("can_send_handshake should be called before sending")
            .crypto_stream
            .tx
            .push(transmission);
    }

    fn can_send_application(&self) -> bool {
        false
    }

    fn send_application(&mut self, _transmission: Bytes) {
        unimplemented!("application level crypto frames cannot currently be sent")
    }
}
