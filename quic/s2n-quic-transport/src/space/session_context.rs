// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection::{self, limits::Limits},
    endpoint,
    recovery::congestion_controller,
    space::{
        rx_packet_numbers::AckManager, ApplicationSpace, HandshakeSpace, HandshakeStatus,
        InitialSpace,
    },
    stream::AbstractStreamManager,
};
use bytes::Bytes;
use s2n_codec::{DecoderBuffer, DecoderValue};
use s2n_quic_core::{
    ack,
    crypto::{tls, CryptoSuite},
    packet::number::PacketNumberSpace,
    path::Path,
    time::Timestamp,
    transport::{error::TransportError, parameters::ClientTransportParameters},
};

pub struct SessionContext<'a, Config: endpoint::Config> {
    pub now: Timestamp,
    pub path: &'a Path<<Config::CongestionControllerEndpoint as congestion_controller::Endpoint>::CongestionController>,
    pub initial: &'a mut Option<Box<InitialSpace<Config>>>,
    pub handshake: &'a mut Option<Box<HandshakeSpace<Config>>>,
    pub application: &'a mut Option<Box<ApplicationSpace<Config>>>,
    pub zero_rtt_crypto: &'a mut Option<
        Box<<<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::ZeroRttKey>,
    >,
    pub handshake_status: &'a mut HandshakeStatus,
    pub local_id_registry: &'a mut connection::LocalIdRegistry,
    pub limits: &'a Limits,
}

impl<'a, Config: endpoint::Config> tls::Context<<Config::TLSEndpoint as tls::Endpoint>::Session>
    for SessionContext<'a, Config>
{
    fn on_handshake_keys(
        &mut self,
        key: <<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::HandshakeKey,
        header_key: <<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::HandshakeHeaderKey,
    ) -> Result<(), TransportError> {
        if self.handshake.is_some() {
            return Err(TransportError::INTERNAL_ERROR
                .with_reason("handshake keys initialized more than once"));
        }

        // After receiving handshake keys, the initial crypto stream should be completely
        // finished
        if let Some(space) = self.initial.as_mut() {
            space.crypto_stream.finish()?;
        }

        let ack_manager = AckManager::new(PacketNumberSpace::Handshake, ack::Settings::EARLY);

        *self.handshake = Some(Box::new(HandshakeSpace::new(
            key,
            header_key,
            self.now,
            ack_manager,
        )));

        Ok(())
    }

    fn on_zero_rtt_keys(
        &mut self,
        key: <<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::ZeroRttKey,
        _header_key: <<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::ZeroRttHeaderKey,
        _application_parameters: tls::ApplicationParameters,
    ) -> Result<(), TransportError> {
        if self.zero_rtt_crypto.is_some() {
            return Err(TransportError::INTERNAL_ERROR
                .with_reason("zero rtt keys initialized more than once"));
        }

        *self.zero_rtt_crypto = Some(Box::new(key));

        Ok(())
    }

    fn on_one_rtt_keys(
        &mut self,
        key: <<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::OneRttKey,
        header_key: <<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::OneRttHeaderKey,
        application_parameters: tls::ApplicationParameters,
    ) -> Result<(), TransportError> {
        if self.application.is_some() {
            return Err(TransportError::INTERNAL_ERROR
                .with_reason("application keys initialized more than once"));
        }

        if Config::ENDPOINT_TYPE.is_client() {
            //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#4.9.3
            //# Therefore, a client SHOULD discard 0-RTT keys as soon as it installs
            //# 1-RTT keys, since they have no use after that moment.

            *self.zero_rtt_crypto = None;
        }

        // Parse transport parameters
        // TODO: This assumes we are a server, and needs to be changed for the client
        let param_decoder = DecoderBuffer::new(application_parameters.transport_parameters);
        let (peer_parameters, remaining) = match ClientTransportParameters::decode(param_decoder) {
            Ok(parameters) => parameters,
            Err(_e) => {
                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#7.4
                //# An endpoint SHOULD treat receipt of
                //# duplicate transport parameters as a connection error of type
                //# TRANSPORT_PARAMETER_ERROR.
                return Err(TransportError::TRANSPORT_PARAMETER_ERROR
                    .with_reason("Invalid transport parameters"));
            }
        };

        if !remaining.is_empty() {
            return Err(TransportError::TRANSPORT_PARAMETER_ERROR
                .with_reason("Invalid bytes in transport parameters"));
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#7.3
        //= type=TODO
        //= feature=Transport parameter ID validation
        //= tracking-issue=353
        //# The values provided by a peer for these transport parameters MUST
        //# match the values that an endpoint used in the Destination and Source
        //# Connection ID fields of Initial packets that it sent.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#7.3
        //= type=TODO
        //= feature=Transport parameter ID validation
        //= tracking-issue=353
        //# An endpoint MUST treat absence of the initial_source_connection_id
        //# transport parameter from either endpoint or absence of the
        //# original_destination_connection_id transport parameter from the
        //# server as a connection error of type TRANSPORT_PARAMETER_ERROR.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#7.3
        //= type=TODO
        //= feature=Transport parameter ID validation
        //= tracking-issue=353
        //# An endpoint MUST treat the following as a connection error of type
        //# TRANSPORT_PARAMETER_ERROR or PROTOCOL_VIOLATION:

        let peer_flow_control_limits = peer_parameters.flow_control_limits();

        let stream_manager = AbstractStreamManager::new(
            &self.limits,
            Config::ENDPOINT_TYPE,
            self.limits.initial_flow_control_limits(),
            peer_flow_control_limits,
        );

        let ack_manager = AckManager::new(
            PacketNumberSpace::ApplicationData,
            self.limits.ack_settings(),
        );

        // TODO use interning for these values
        // issue: https://github.com/awslabs/s2n-quic/issues/248
        let sni = application_parameters.sni.map(Bytes::copy_from_slice);
        let alpn = Bytes::copy_from_slice(application_parameters.alpn_protocol);

        *self.application = Some(Box::new(ApplicationSpace::new(
            key,
            header_key,
            self.now,
            stream_manager,
            ack_manager,
            sni,
            alpn,
        )));

        self.local_id_registry
            .set_active_connection_id_limit(peer_parameters.active_connection_id_limit.as_u64());

        Ok(())
    }

    fn on_handshake_done(&mut self) -> Result<(), TransportError> {
        // After the handshake is done, the handshake crypto stream should be completely
        // finished
        if let Some(space) = self.handshake.as_mut() {
            space.crypto_stream.finish()?;
        }

        if let Some(application) = self.application.as_mut() {
            if Config::ENDPOINT_TYPE.is_server() {
                //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#4.9.2
                //# The server MUST send a HANDSHAKE_DONE
                //# frame as soon as it completes the handshake.
                self.handshake_status.on_handshake_done();

                // All of the other spaces are discarded by the time the handshake is confirmed so
                // we only need to notify the application space
                application.on_handshake_done(&self.path, self.local_id_registry, self.now);
            }
            Ok(())
        } else {
            Err(TransportError::INTERNAL_ERROR
                .with_reason("handshake cannot be completed without application keys"))
        }
    }

    fn receive_initial(&mut self, max_len: Option<usize>) -> Option<Bytes> {
        self.initial
            .as_mut()
            .map(Box::as_mut)?
            .crypto_stream
            .rx
            .pop_watermarked(max_len.unwrap_or(usize::MAX))
            .map(|bytes| bytes.freeze())
    }

    fn receive_handshake(&mut self, max_len: Option<usize>) -> Option<Bytes> {
        self.handshake
            .as_mut()
            .map(Box::as_mut)?
            .crypto_stream
            .rx
            .pop_watermarked(max_len.unwrap_or(usize::MAX))
            .map(|bytes| bytes.freeze())
    }

    fn receive_application(&mut self, _max_len: Option<usize>) -> Option<Bytes> {
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
