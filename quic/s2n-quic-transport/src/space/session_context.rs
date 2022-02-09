// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection::{self, limits::Limits},
    endpoint, path,
    space::{
        keep_alive::KeepAlive, rx_packet_numbers::AckManager, ApplicationSpace, HandshakeSpace,
        HandshakeStatus, InitialSpace,
    },
    stream::AbstractStreamManager,
};
use bytes::Bytes;
use core::ops::Not;
use s2n_codec::{DecoderBuffer, DecoderValue};
use s2n_quic_core::{
    ack,
    connection::{InitialId, PeerId},
    crypto::{tls, CryptoSuite, Key},
    ct::ConstantTimeEq,
    event,
    event::IntoEvent,
    packet::number::PacketNumberSpace,
    time::Timestamp,
    transport::{
        self,
        parameters::{
            ActiveConnectionIdLimit, ClientTransportParameters, InitialFlowControlLimits,
            InitialSourceConnectionId, ServerTransportParameters,
        },
    },
};

pub struct SessionContext<'a, Config: endpoint::Config, Pub: event::ConnectionPublisher> {
    pub now: Timestamp,
    pub initial_cid: &'a InitialId,
    pub retry_cid: Option<&'a PeerId>,
    pub path_manager: &'a mut path::Manager<Config>,
    pub initial: &'a mut Option<Box<InitialSpace<Config>>>,
    pub handshake: &'a mut Option<Box<HandshakeSpace<Config>>>,
    pub application: &'a mut Option<Box<ApplicationSpace<Config>>>,
    pub zero_rtt_crypto: &'a mut Option<
        Box<<<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::ZeroRttKey>,
    >,
    pub handshake_status: &'a mut HandshakeStatus,
    pub local_id_registry: &'a mut connection::LocalIdRegistry,
    pub limits: &'a mut Limits,
    pub publisher: &'a mut Pub,
}

impl<'a, Config: endpoint::Config, Pub: event::ConnectionPublisher>
    SessionContext<'a, Config, Pub>
{
    // This is called by the client
    fn on_server_params(
        &mut self,
        decoder: DecoderBuffer,
    ) -> Result<(InitialFlowControlLimits, ActiveConnectionIdLimit), transport::Error> {
        debug_assert!(Config::ENDPOINT_TYPE.is_client());

        let (peer_parameters, remaining) =
            ServerTransportParameters::decode(decoder).map_err(|_| {
                //= https://www.rfc-editor.org/rfc/rfc9000#7.4
                //# An endpoint SHOULD treat receipt of
                //# duplicate transport parameters as a connection error of type
                //# TRANSPORT_PARAMETER_ERROR.
                transport::Error::TRANSPORT_PARAMETER_ERROR
                    .with_reason("Invalid transport parameters")
            })?;

        debug_assert_eq!(remaining.len(), 0);
        self.publisher.on_transport_parameters_received(
            event::builder::TransportParametersReceived {
                transport_parameters: peer_parameters.into_event(),
            },
        );

        //= https://www.rfc-editor.org/rfc/rfc9000#7.3
        //# An endpoint MUST treat the following as a connection error of type
        //# TRANSPORT_PARAMETER_ERROR or PROTOCOL_VIOLATION:
        self.validate_initial_source_connection_id(
            &peer_parameters.initial_source_connection_id,
            self.path_manager
                .active_path()
                .peer_connection_id
                .as_bytes(),
        )?;

        match (self.retry_cid, peer_parameters.retry_source_connection_id) {
            (Some(retry_packet_value), Some(transport_params_value)) => {
                if retry_packet_value
                    .as_bytes()
                    .ct_eq(transport_params_value.as_bytes())
                    .not()
                    .into()
                {
                    return Err(transport::Error::TRANSPORT_PARAMETER_ERROR
                        .with_reason("retry_source_connection_id mismatch"));
                }
            }
            (Some(_), None) => {
                //= https://www.rfc-editor.org/rfc/rfc9000#7.3
                //# *  absence of the retry_source_connection_id transport parameter from
                //# the server after receiving a Retry packet,
                return Err(transport::Error::TRANSPORT_PARAMETER_ERROR.with_reason(
                    "retry_source_connection_id transport parameter absent \
                    after receiving a Retry packet from the server",
                ));
            }
            (None, Some(_)) => {
                //= https://www.rfc-editor.org/rfc/rfc9000#7.3
                //# *  presence of the retry_source_connection_id transport parameter
                //# when no Retry packet was received, or
                return Err(transport::Error::TRANSPORT_PARAMETER_ERROR.with_reason(
                    "retry_source_connection_id transport parameter present \
                    when no Retry packet was received",
                ));
            }
            (None, None) => {}
        }

        if let Some(peer_value) = peer_parameters.original_destination_connection_id {
            //= https://www.rfc-editor.org/rfc/rfc9000#7.3
            //# The values provided by a peer for these transport parameters MUST
            //# match the values that an endpoint used in the Destination and Source
            //# Connection ID fields of Initial packets that it sent (and received,
            //# for servers).  Endpoints MUST validate that received transport
            //# parameters match received connection ID values.
            if peer_value
                .as_bytes()
                .ct_eq(self.initial_cid.as_bytes())
                .not()
                .into()
            {
                return Err(transport::Error::TRANSPORT_PARAMETER_ERROR
                    .with_reason("original_destination_connection_id mismatch"));
            }
        } else {
            //= https://www.rfc-editor.org/rfc/rfc9000#7.3
            //# An endpoint MUST treat the absence of the
            //# initial_source_connection_id transport parameter from either endpoint
            //# or the absence of the original_destination_connection_id transport
            //# parameter from the server as a connection error of type
            //# TRANSPORT_PARAMETER_ERROR.
            return Err(transport::Error::TRANSPORT_PARAMETER_ERROR
                .with_reason("missing original_destination_connection_id"));
        }

        //= https://www.rfc-editor.org/rfc/rfc9000#10.3
        //# Servers can also issue a stateless_reset_token transport parameter during the
        //# handshake that applies to the connection ID that it selected during
        //# the handshake.  These exchanges are protected by encryption, so only
        //# client and server know their value.  Note that clients cannot use the
        //# stateless_reset_token transport parameter because their transport
        //# parameters do not have confidentiality protection.
        if let Some(stateless_reset_token) = peer_parameters.stateless_reset_token {
            self.path_manager
                .peer_id_registry
                .register_initial_stateless_reset_token(stateless_reset_token);
        }

        // Load the peer's transport parameters into the connection's limits
        self.limits.load_peer(&peer_parameters);

        let initial_flow_control_limits = peer_parameters.flow_control_limits();
        let active_connection_id_limit = peer_parameters.active_connection_id_limit;

        Ok((initial_flow_control_limits, active_connection_id_limit))
    }

    // This is called by the server
    fn on_client_params(
        &mut self,
        decoder: DecoderBuffer,
    ) -> Result<(InitialFlowControlLimits, ActiveConnectionIdLimit), transport::Error> {
        debug_assert!(Config::ENDPOINT_TYPE.is_server());

        let (peer_parameters, remaining) =
            ClientTransportParameters::decode(decoder).map_err(|_| {
                //= https://www.rfc-editor.org/rfc/rfc9000#7.4
                //# An endpoint SHOULD treat receipt of
                //# duplicate transport parameters as a connection error of type
                //# TRANSPORT_PARAMETER_ERROR.
                transport::Error::TRANSPORT_PARAMETER_ERROR
                    .with_reason("Invalid transport parameters")
            })?;

        debug_assert_eq!(remaining.len(), 0);
        self.publisher.on_transport_parameters_received(
            event::builder::TransportParametersReceived {
                transport_parameters: peer_parameters.into_event(),
            },
        );

        //= https://www.rfc-editor.org/rfc/rfc9000#7.3
        //# An endpoint MUST treat the following as a connection error of type
        //# TRANSPORT_PARAMETER_ERROR or PROTOCOL_VIOLATION:
        self.validate_initial_source_connection_id(
            &peer_parameters.initial_source_connection_id,
            self.path_manager
                .active_path()
                .peer_connection_id
                .as_bytes(),
        )?;

        // Load the peer's transport parameters into the connection's limits
        self.limits.load_peer(&peer_parameters);

        let initial_flow_control_limits = peer_parameters.flow_control_limits();
        let active_connection_id_limit = peer_parameters.active_connection_id_limit;

        Ok((initial_flow_control_limits, active_connection_id_limit))
    }

    //= https://www.rfc-editor.org/rfc/rfc9000#7.3
    //# Each endpoint includes the value of the Source Connection ID field
    //# from the first Initial packet it sent in the
    //# initial_source_connection_id transport parameter
    //
    // When the endpoint is a Server this is the peer's connection id.
    //
    // When the endpoint is a Client, this is the randomly generated
    // initial_connection_id which is locally generated for the first Initial packet.
    fn validate_initial_source_connection_id(
        &self,
        peer_value: &Option<InitialSourceConnectionId>,
        expected_value: &[u8],
    ) -> Result<(), transport::Error> {
        //= https://www.rfc-editor.org/rfc/rfc9000#7.3
        //# * a mismatch between values received from a peer in these transport
        //# parameters and the value sent in the corresponding Destination or
        //# Source Connection ID fields of Initial packets.
        if let Some(peer_value) = peer_value {
            //= https://www.rfc-editor.org/rfc/rfc9000#7.3
            //# The values provided by a peer for these transport parameters MUST
            //# match the values that an endpoint used in the Destination and Source
            //# Connection ID fields of Initial packets that it sent (and received,
            //# for servers).  Endpoints MUST validate that received transport
            //# parameters match received connection ID values.
            if peer_value.as_bytes().ct_eq(expected_value).not().into() {
                return Err(transport::Error::TRANSPORT_PARAMETER_ERROR
                    .with_reason("initial_source_connection_id mismatch"));
            }
        } else {
            //= https://www.rfc-editor.org/rfc/rfc9000#7.3
            //# An endpoint MUST treat the absence of the
            //# initial_source_connection_id transport parameter from either endpoint
            //# or the absence of the original_destination_connection_id transport
            //# parameter from the server as a connection error of type
            //# TRANSPORT_PARAMETER_ERROR.
            return Err(transport::Error::TRANSPORT_PARAMETER_ERROR
                .with_reason("missing initial_source_connection_id"));
        }

        Ok(())
    }
}

impl<'a, Config: endpoint::Config, Pub: event::ConnectionPublisher>
    tls::Context<<Config::TLSEndpoint as tls::Endpoint>::Session>
    for SessionContext<'a, Config, Pub>
{
    fn on_handshake_keys(
        &mut self,
        key: <<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::HandshakeKey,
        header_key: <<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::HandshakeHeaderKey,
    ) -> Result<(), transport::Error> {
        if self.handshake.is_some() {
            return Err(transport::Error::INTERNAL_ERROR
                .with_reason("handshake keys initialized more than once"));
        }

        // After receiving handshake keys, the initial crypto stream should be completely
        // finished
        if let Some(space) = self.initial.as_mut() {
            space.crypto_stream.finish()?;
        }

        let ack_manager = AckManager::new(PacketNumberSpace::Handshake, ack::Settings::EARLY);

        let cipher_suite = key.cipher_suite().into_event();
        *self.handshake = Some(Box::new(HandshakeSpace::new(
            key,
            header_key,
            self.now,
            ack_manager,
        )));
        self.publisher.on_key_update(event::builder::KeyUpdate {
            key_type: event::builder::KeyType::Handshake,
            cipher_suite,
        });
        Ok(())
    }

    fn on_zero_rtt_keys(
        &mut self,
        key: <<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::ZeroRttKey,
        _header_key: <<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::ZeroRttHeaderKey,
        _application_parameters: tls::ApplicationParameters,
    ) -> Result<(), transport::Error> {
        if self.zero_rtt_crypto.is_some() {
            return Err(transport::Error::INTERNAL_ERROR
                .with_reason("zero rtt keys initialized more than once"));
        }

        let cipher_suite = key.cipher_suite().into_event();

        // TODO: also store the header_key https://github.com/awslabs/s2n-quic/issues/319
        *self.zero_rtt_crypto = Some(Box::new(key));

        self.publisher.on_key_update(event::builder::KeyUpdate {
            key_type: event::builder::KeyType::ZeroRtt,
            cipher_suite,
        });
        Ok(())
    }

    fn on_one_rtt_keys(
        &mut self,
        key: <<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::OneRttKey,
        header_key: <<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::OneRttHeaderKey,
        application_parameters: tls::ApplicationParameters,
    ) -> Result<(), transport::Error> {
        if self.application.is_some() {
            return Err(transport::Error::INTERNAL_ERROR
                .with_reason("application keys initialized more than once"));
        }

        if Config::ENDPOINT_TYPE.is_client() {
            //= https://www.rfc-editor.org/rfc/rfc9001#4.9.3
            //# Therefore, a client SHOULD discard 0-RTT keys as soon as it installs
            //# 1-RTT keys as they have no use after that moment.

            *self.zero_rtt_crypto = None;
        }

        // Parse transport parameters
        let param_decoder = DecoderBuffer::new(application_parameters.transport_parameters);
        let (peer_flow_control_limits, active_connection_id_limit) = match Config::ENDPOINT_TYPE {
            endpoint::Type::Client => self.on_server_params(param_decoder)?,
            endpoint::Type::Server => self.on_client_params(param_decoder)?,
        };

        self.local_id_registry
            .set_active_connection_id_limit(active_connection_id_limit.as_u64());

        let stream_manager = AbstractStreamManager::new(
            self.limits,
            Config::ENDPOINT_TYPE,
            self.limits.initial_flow_control_limits(),
            peer_flow_control_limits,
        );

        let ack_manager = AckManager::new(
            PacketNumberSpace::ApplicationData,
            self.limits.ack_settings(),
        );

        let keep_alive = KeepAlive::new(
            self.limits.max_idle_timeout(),
            self.limits.max_keep_alive_period(),
        );

        // TODO use interning for these values
        // issue: https://github.com/awslabs/s2n-quic/issues/248
        let server_name = application_parameters.server_name;
        let application_protocol =
            Bytes::copy_from_slice(application_parameters.application_protocol);

        self.publisher
            .on_alpn_information(event::builder::AlpnInformation {
                chosen_alpn: &application_protocol,
            });
        self.publisher.on_application_protocol_information(
            event::builder::ApplicationProtocolInformation {
                chosen_application_protocol: &application_protocol,
            },
        );
        if let Some(chosen_server_name) = &server_name {
            self.publisher
                .on_sni_information(event::builder::SniInformation {
                    chosen_sni: chosen_server_name,
                });
            self.publisher
                .on_server_name_information(event::builder::ServerNameInformation {
                    chosen_server_name,
                });
        };

        let cipher_suite = key.cipher_suite().into_event();
        *self.application = Some(Box::new(ApplicationSpace::new(
            key,
            header_key,
            self.now,
            stream_manager,
            ack_manager,
            keep_alive,
            server_name,
            application_protocol,
        )));
        self.publisher.on_key_update(event::builder::KeyUpdate {
            key_type: event::builder::KeyType::OneRtt { generation: 0 },
            cipher_suite,
        });

        Ok(())
    }

    fn on_handshake_complete(&mut self) -> Result<(), transport::Error> {
        // After the handshake is complete, the handshake crypto stream should be completely
        // finished
        if let Some(space) = self.handshake.as_mut() {
            space.crypto_stream.finish()?;
        }

        self.handshake_status
            .on_handshake_complete(Config::ENDPOINT_TYPE, self.publisher);

        if let Some(application) = self.application.as_mut() {
            if Config::ENDPOINT_TYPE.is_server() {
                // All of the other spaces are discarded by the time the handshake is complete so
                // we only need to notify the application space
                //
                //= https://www.rfc-editor.org/rfc/rfc9001#4.1.2
                //# the TLS handshake is considered confirmed at the
                //# server when the handshake completes.
                application.on_handshake_confirmed(
                    self.path_manager.active_path(),
                    self.local_id_registry,
                    self.now,
                );
            }
            Ok(())
        } else {
            Err(transport::Error::INTERNAL_ERROR
                .with_reason("handshake cannot be completed without application keys"))
        }
    }

    fn receive_initial(&mut self, max_len: Option<usize>) -> Option<Bytes> {
        let space = self.initial.as_deref_mut()?;

        // don't pass the buffer until we have a full hello message
        if !space.received_hello_message {
            return None;
        }

        space
            .crypto_stream
            .rx
            .pop_watermarked(max_len.unwrap_or(usize::MAX))
            .map(|bytes| bytes.freeze())
    }

    fn receive_handshake(&mut self, max_len: Option<usize>) -> Option<Bytes> {
        self.handshake
            .as_deref_mut()?
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
