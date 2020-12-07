use crate::{
    connection,
    connection::ConnectionIdMapperRegistration,
    space::{
        rx_packet_numbers::{AckManager, DEFAULT_ACK_RANGES_LIMIT, EARLY_ACK_SETTINGS},
        ApplicationSpace, HandshakeSpace, HandshakeStatus, InitialSpace,
    },
    stream::AbstractStreamManager,
};
use bytes::Bytes;
use s2n_codec::{DecoderBuffer, DecoderValue};
use s2n_quic_core::{
    crypto::{tls, CryptoError, CryptoSuite},
    packet::number::PacketNumberSpace,
    path::Path,
    time::Timestamp,
    transport::parameters::ClientTransportParameters,
};

pub struct SessionContext<'a, Config: connection::Config> {
    pub now: Timestamp,
    pub connection_config: &'a Config,
    pub path: &'a Path<Config::CongestionController>,
    pub initial: &'a mut Option<Box<InitialSpace<Config>>>,
    pub handshake: &'a mut Option<Box<HandshakeSpace<Config>>>,
    pub application: &'a mut Option<Box<ApplicationSpace<Config>>>,
    pub zero_rtt_crypto: &'a mut Option<Box<<Config::TLSSession as CryptoSuite>::ZeroRTTCrypto>>,
    pub handshake_status: &'a mut HandshakeStatus,
    pub connection_id_mapper_registration: &'a mut ConnectionIdMapperRegistration,
}

impl<'a, Config: connection::Config> tls::Context<Config::TLSSession>
    for SessionContext<'a, Config>
{
    fn on_handshake_keys(
        &mut self,
        keys: <Config::TLSSession as CryptoSuite>::HandshakeCrypto,
    ) -> Result<(), CryptoError> {
        if self.handshake.is_some() {
            return Err(CryptoError::INTERNAL_ERROR
                .with_reason("handshake keys initialized more than once"));
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
        keys: <Config::TLSSession as CryptoSuite>::ZeroRTTCrypto,
        _application_parameters: tls::ApplicationParameters,
    ) -> Result<(), CryptoError> {
        if self.zero_rtt_crypto.is_some() {
            return Err(
                CryptoError::INTERNAL_ERROR.with_reason("zero rtt keys initialized more than once")
            );
        }

        *self.zero_rtt_crypto = Some(Box::new(keys));

        Ok(())
    }

    fn on_one_rtt_keys(
        &mut self,
        keys: <Config::TLSSession as CryptoSuite>::OneRTTCrypto,
        application_parameters: tls::ApplicationParameters,
    ) -> Result<(), CryptoError> {
        if self.application.is_some() {
            return Err(CryptoError::INTERNAL_ERROR
                .with_reason("application keys initialized more than once"));
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

        // TODO authenticate initial_source_connection_id
        // TODO authenticate original_destination_connection_id
        // https://tools.ietf.org/html/draft-ietf-quic-transport-29#section-7.3

        let peer_flow_control_limits = peer_parameters.flow_control_limits();
        let local_flow_control_limits = self.connection_config.local_flow_control_limits();
        let connection_limits = self.connection_config.connection_limits();

        let stream_manager = AbstractStreamManager::new(
            &connection_limits,
            Config::ENDPOINT_TYPE,
            local_flow_control_limits,
            peer_flow_control_limits,
        );

        // TODO ack interval limit configurable
        let ack_interval_limit = DEFAULT_ACK_RANGES_LIMIT;
        let ack_settings = self.connection_config.local_ack_settings();
        let ack_manager = AckManager::new(
            PacketNumberSpace::ApplicationData,
            ack_settings,
            ack_interval_limit,
        );

        // TODO use interning for these values
        // issue: https://github.com/awslabs/s2n-quic/issues/248
        let sni = application_parameters.sni.map(Bytes::copy_from_slice);
        let alpn = application_parameters
            .alpn_protocol
            .map(Bytes::copy_from_slice);

        *self.application = Some(Box::new(ApplicationSpace::new(
            keys,
            self.now,
            stream_manager,
            ack_manager,
            sni,
            alpn,
        )));

        self.connection_id_mapper_registration
            .set_active_connection_id_limit(peer_parameters.active_connection_id_limit.as_u64());

        Ok(())
    }

    fn on_handshake_done(&mut self) -> Result<(), CryptoError> {
        if let Some(application) = self.application.as_mut() {
            if Config::ENDPOINT_TYPE.is_server() {
                //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#4.9.2
                //# The server MUST send a HANDSHAKE_DONE
                //# frame as soon as it completes the handshake.
                self.handshake_status.on_handshake_done();

                // All of the other spaces are discarded by the time the handshake is confirmed so
                // we only need to notify the application space
                application.on_handshake_done(
                    &self.path,
                    self.connection_id_mapper_registration,
                    self.now,
                );
            }
            Ok(())
        } else {
            Err(CryptoError::INTERNAL_ERROR
                .with_reason("handshake cannot be completed without application keys"))
        }
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
        if let Some(initial) = self.initial.as_mut() {
            initial.crypto_stream.finish();
        }

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
