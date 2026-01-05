// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    ack::AckManager,
    connection::{self, limits::Limits},
    endpoint, path,
    space::{
        datagram, keep_alive::KeepAlive, ApplicationSpace, HandshakeSpace, HandshakeStatus,
        InitialSpace,
    },
    stream,
};
use bytes::Bytes;
use core::{any::Any, ops::Not, task::Waker};
use s2n_codec::{DecoderBuffer, DecoderValue};
use s2n_quic_core::{
    ack,
    application::ServerName,
    connection::{
        limits::{HandshakeInfo, Limiter, UpdatableLimits},
        InitialId, PeerId,
    },
    crypto::{
        self,
        tls::{self, ApplicationParameters, NamedGroup},
        CryptoSuite, Key,
    },
    ct::ConstantTimeEq,
    datagram::{ConnectionInfo, Endpoint},
    dc::{self, Endpoint as _},
    event::{
        self,
        builder::{DcPathCreated, DcState, DcStateChanged},
        IntoEvent,
    },
    packet::number::PacketNumberSpace,
    time::Timestamp,
    transport::{
        self,
        parameters::{
            ActiveConnectionIdLimit, ClientTransportParameters, DatagramLimits,
            DcSupportedVersions, InitialFlowControlLimits, InitialSourceConnectionId, MaxAckDelay,
            MtuProbingCompleteSupport, ServerTransportParameters, TransportParameter as _,
        },
        Error,
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
    pub server_name: &'a mut Option<ServerName>,
    pub application_protocol: &'a mut Bytes,
    pub waker: &'a Waker,
    pub publisher: &'a mut Pub,
    pub datagram: &'a mut Config::DatagramEndpoint,
    pub dc: &'a mut Config::DcEndpoint,
    pub limits_endpoint: &'a mut Config::ConnectionLimits,
    pub tls_context: &'a mut Option<Box<dyn Any + Send>>,
    pub random_generator: &'a mut Config::RandomGenerator,
}

impl<Config: endpoint::Config, Pub: event::ConnectionPublisher> SessionContext<'_, Config, Pub> {
    // This is called by the client
    fn on_server_params(
        &mut self,
        decoder: DecoderBuffer,
    ) -> Result<
        (
            InitialFlowControlLimits,
            ActiveConnectionIdLimit,
            DatagramLimits,
            MaxAckDelay,
            Option<dc::Version>,
        ),
        transport::Error,
    > {
        debug_assert!(Config::ENDPOINT_TYPE.is_client());

        let (peer_parameters, remaining) =
            ServerTransportParameters::decode(decoder).map_err(|_| {
                //= https://www.rfc-editor.org/rfc/rfc9000#section-7.4
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

        //= https://www.rfc-editor.org/rfc/rfc9000#section-7.3
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
                //= https://www.rfc-editor.org/rfc/rfc9000#section-7.3
                //# *  absence of the retry_source_connection_id transport parameter from
                //# the server after receiving a Retry packet,
                return Err(transport::Error::TRANSPORT_PARAMETER_ERROR.with_reason(
                    "retry_source_connection_id transport parameter absent \
                    after receiving a Retry packet from the server",
                ));
            }
            (None, Some(_)) => {
                //= https://www.rfc-editor.org/rfc/rfc9000#section-7.3
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
            //= https://www.rfc-editor.org/rfc/rfc9000#section-7.3
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
            //= https://www.rfc-editor.org/rfc/rfc9000#section-7.3
            //# An endpoint MUST treat the absence of the
            //# initial_source_connection_id transport parameter from either endpoint
            //# or the absence of the original_destination_connection_id transport
            //# parameter from the server as a connection error of type
            //# TRANSPORT_PARAMETER_ERROR.
            return Err(transport::Error::TRANSPORT_PARAMETER_ERROR
                .with_reason("missing original_destination_connection_id"));
        }

        //= https://www.rfc-editor.org/rfc/rfc9000#section-10.3
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
        let datagram_limits = peer_parameters.datagram_limits();

        let dc_version = if Config::DcEndpoint::ENABLED {
            peer_parameters
                .dc_supported_versions
                .selected_version()
                .map_err(|_| {
                    transport::Error::TRANSPORT_PARAMETER_ERROR
                        .with_reason("invalid dc supported versions")
                })?
        } else {
            None
        };

        Ok((
            initial_flow_control_limits,
            active_connection_id_limit,
            datagram_limits,
            peer_parameters.max_ack_delay,
            dc_version,
        ))
    }

    // This is called by the server
    fn on_client_params(
        &mut self,
        decoder: DecoderBuffer,
    ) -> Result<
        (
            InitialFlowControlLimits,
            ActiveConnectionIdLimit,
            DatagramLimits,
            MaxAckDelay,
            Option<dc::Version>,
        ),
        transport::Error,
    > {
        debug_assert!(Config::ENDPOINT_TYPE.is_server());

        let (peer_parameters, remaining) =
            ClientTransportParameters::decode(decoder).map_err(|_| {
                //= https://www.rfc-editor.org/rfc/rfc9000#section-7.4
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

        //= https://www.rfc-editor.org/rfc/rfc9000#section-7.3
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
        let datagram_limits = peer_parameters.datagram_limits();

        let dc_version = if Config::DcEndpoint::ENABLED {
            dc::select_version(peer_parameters.dc_supported_versions)
        } else {
            None
        };

        Ok((
            initial_flow_control_limits,
            active_connection_id_limit,
            datagram_limits,
            peer_parameters.max_ack_delay,
            dc_version,
        ))
    }

    //= https://www.rfc-editor.org/rfc/rfc9000#section-7.3
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
        //= https://www.rfc-editor.org/rfc/rfc9000#section-7.3
        //# * a mismatch between values received from a peer in these transport
        //# parameters and the value sent in the corresponding Destination or
        //# Source Connection ID fields of Initial packets.
        if let Some(peer_value) = peer_value {
            //= https://www.rfc-editor.org/rfc/rfc9000#section-7.3
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
            //= https://www.rfc-editor.org/rfc/rfc9000#section-7.3
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

impl<Config: endpoint::Config, Pub: event::ConnectionPublisher>
    tls::Context<<Config::TLSEndpoint as tls::Endpoint>::Session>
    for SessionContext<'_, Config, Pub>
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

        // TODO: also store the header_key
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
            //= https://www.rfc-editor.org/rfc/rfc9001#section-4.9.3
            //# Therefore, a client SHOULD discard 0-RTT keys as soon as it installs
            //# 1-RTT keys as they have no use after that moment.

            *self.zero_rtt_crypto = None;
        }

        // Parse transport parameters
        let param_decoder = DecoderBuffer::new(application_parameters.transport_parameters);
        let (
            peer_flow_control_limits,
            active_connection_id_limit,
            datagram_limits,
            max_ack_delay,
            dc_version,
        ) = match Config::ENDPOINT_TYPE {
            endpoint::Type::Client => self.on_server_params(param_decoder)?,
            endpoint::Type::Server => self.on_client_params(param_decoder)?,
        };

        let remote_address = self.path_manager.active_path().remote_address().0;
        let info = HandshakeInfo::new(
            &remote_address,
            self.server_name.as_ref(),
            self.application_protocol,
        );
        let mut updatable_limits = UpdatableLimits::new(self.limits);
        self.limits_endpoint
            .on_post_handshake(&info, &mut updatable_limits);

        self.local_id_registry
            .set_active_connection_id_limit(active_connection_id_limit.as_u64());

        let stream_manager = <Config::StreamManager as stream::Manager>::new(
            self.limits,
            Config::ENDPOINT_TYPE,
            self.limits.initial_flow_control_limits(),
            peer_flow_control_limits,
            self.path_manager.active_path().rtt_estimator.min_rtt(),
        );

        let ack_manager = AckManager::new(
            PacketNumberSpace::ApplicationData,
            self.limits.ack_settings(),
        );

        let keep_alive = KeepAlive::new(
            self.limits.max_idle_timeout(),
            self.limits.max_keep_alive_period(),
        );

        let conn_info =
            ConnectionInfo::new(datagram_limits.max_datagram_payload, self.waker.clone());
        let (datagram_sender, datagram_receiver) = self.datagram.create_connection(&conn_info);
        let datagram_manager = datagram::Manager::new(
            datagram_sender,
            datagram_receiver,
            datagram_limits.max_datagram_payload,
        );

        let dc_manager = if let Some(dc_version) = dc_version {
            let application_params = dc::ApplicationParams::new(
                self.path_manager
                    .active_path()
                    .mtu_controller
                    .max_datagram_size() as u16,
                &peer_flow_control_limits,
                self.limits,
            );
            let remote_address = self.path_manager.active_path().remote_address().0;
            let conn_info = dc::ConnectionInfo::new(
                &remote_address,
                dc_version,
                application_params,
                Config::ENDPOINT_TYPE.into_event(),
            );
            let dc_path = self.dc.new_path(&conn_info);

            // &mut would be ideal but events currently need to be `Clone`, and we're OK with
            // pushing interior mutability for now. dc is all unstable anyway.
            self.publisher
                .on_dc_path_created(DcPathCreated { path: &dc_path });

            if self.dc.mtu_probing_complete_support() {
                self.path_manager
                    .active_path_mut()
                    .mtu_controller
                    .enable_mtu_probing_complete_support();
            }

            crate::dc::Manager::new(dc_path, dc_version, self.publisher)
        } else {
            if Config::DcEndpoint::ENABLED {
                self.publisher.on_dc_state_changed(DcStateChanged {
                    state: DcState::NoVersionNegotiated,
                });
            }
            crate::dc::Manager::disabled()
        };

        self.path_manager
            .active_path_mut()
            .rtt_estimator
            .on_max_ack_delay(max_ack_delay);

        let cipher_suite = key.cipher_suite().into_event();
        *self.application = Some(Box::new(ApplicationSpace::new(
            key,
            header_key,
            self.now,
            stream_manager,
            ack_manager,
            keep_alive,
            datagram_manager,
            dc_manager,
        )));
        self.publisher.on_key_update(event::builder::KeyUpdate {
            key_type: event::builder::KeyType::OneRtt { generation: 0 },
            cipher_suite,
        });

        Ok(())
    }

    fn on_server_name(&mut self, server_name: ServerName) -> Result<(), transport::Error> {
        self.publisher
            .on_server_name_information(event::builder::ServerNameInformation {
                chosen_server_name: &server_name,
            });
        *self.server_name = Some(server_name);

        Ok(())
    }

    fn on_application_protocol(
        &mut self,
        application_protocol: Bytes,
    ) -> Result<(), transport::Error> {
        self.publisher.on_application_protocol_information(
            event::builder::ApplicationProtocolInformation {
                chosen_application_protocol: &application_protocol,
            },
        );
        *self.application_protocol = application_protocol;

        Ok(())
    }

    fn on_key_exchange_group(&mut self, named_group: NamedGroup) -> Result<(), transport::Error> {
        self.publisher
            .on_key_exchange_group(event::builder::KeyExchangeGroup {
                chosen_group_name: named_group.group_name,
                contains_kem: named_group.contains_kem,
            });

        Ok(())
    }

    fn on_tls_exporter_ready(
        &mut self,
        session: &impl tls::TlsSession,
    ) -> Result<(), transport::Error> {
        self.application
            .as_mut()
            .expect("application keys should be ready before the tls exporter")
            .dc_manager
            .on_path_secrets_ready(session, self.publisher)?;

        self.publisher
            .on_tls_exporter_ready(event::builder::TlsExporterReady {
                session: s2n_quic_core::event::TlsSession::new(session),
            });
        Ok(())
    }

    fn on_tls_handshake_failed(
        &mut self,
        session: &impl tls::TlsSession,
        e: &(dyn std::error::Error + Send + Sync + 'static),
    ) -> Result<(), transport::Error> {
        self.publisher
            .on_tls_handshake_failed(event::builder::TlsHandshakeFailed {
                session: s2n_quic_core::event::TlsSession::new(session),
                error: e,
            });
        Ok(())
    }

    fn on_handshake_complete(&mut self) -> Result<(), transport::Error> {
        // After the handshake is complete, the handshake crypto stream should be completely
        // finished
        if let Some(space) = self.handshake.as_mut() {
            space.crypto_stream.finish()?;
        }

        if self.application_protocol.is_empty() {
            //= https://www.rfc-editor.org/rfc/rfc9001#section-8.1
            //# When using ALPN, endpoints MUST immediately close a connection (see
            //# Section 10.2 of [QUIC-TRANSPORT]) with a no_application_protocol TLS
            //# alert (QUIC error code 0x178; see Section 4.8) if an application
            //# protocol is not negotiated.

            //= https://www.rfc-editor.org/rfc/rfc9001#section-8.1
            //# While [ALPN] only specifies that servers
            //# use this alert, QUIC clients MUST use error 0x178 to terminate a
            //# connection when ALPN negotiation fails.
            let err = crypto::tls::Error::NO_APPLICATION_PROTOCOL
                .with_reason("Missing ALPN protocol")
                .into();
            return Err(err);
        }

        self.handshake_status
            .on_handshake_complete(Config::ENDPOINT_TYPE, self.publisher);

        if let Some(application) = self.application.as_mut() {
            if Config::ENDPOINT_TYPE.is_server() {
                // All of the other spaces are discarded by the time the handshake is complete so
                // we only need to notify the application space
                //
                //= https://www.rfc-editor.org/rfc/rfc9001#section-4.1.2
                //# the TLS handshake is considered confirmed at the
                //# server when the handshake completes.
                application.on_handshake_confirmed(
                    self.path_manager.active_path(),
                    self.local_id_registry,
                    self.random_generator,
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

    fn receive_application(&mut self, max_len: Option<usize>) -> Option<Bytes> {
        self.application
            .as_deref_mut()?
            .crypto_stream
            .rx
            .pop_watermarked(max_len.unwrap_or(usize::MAX))
            .map(|bytes| bytes.freeze())
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
        self.application
            .as_ref()
            .map(|space| space.crypto_stream.can_send())
            .unwrap_or_default()
    }

    fn send_application(&mut self, transmission: Bytes) {
        if cfg!(any(test, feature = "unstable_resumption")) {
            self.application
                .as_mut()
                .expect("can_send_application should be called before sending")
                .crypto_stream
                .tx
                .push(transmission);
        }
    }

    fn waker(&self) -> &Waker {
        self.waker
    }

    fn on_client_application_params(
        &mut self,
        client_params: ApplicationParameters,
        server_params: &mut Vec<u8>,
    ) -> Result<(), Error> {
        debug_assert!(Config::ENDPOINT_TYPE.is_server());

        if Config::DcEndpoint::ENABLED {
            let param_decoder = DecoderBuffer::new(client_params.transport_parameters);
            let (client_params, remaining) = ClientTransportParameters::decode(param_decoder)
                .map_err(|_| {
                    //= https://www.rfc-editor.org/rfc/rfc9000#section-7.4
                    //# An endpoint SHOULD treat receipt of
                    //# duplicate transport parameters as a connection error of type
                    //# TRANSPORT_PARAMETER_ERROR.
                    transport::Error::TRANSPORT_PARAMETER_ERROR
                        .with_reason("Invalid transport parameters")
                })?;

            debug_assert_eq!(remaining.len(), 0);

            if let Some(selected_version) = dc::select_version(client_params.dc_supported_versions)
            {
                DcSupportedVersions::for_server(selected_version).append_to_buffer(server_params)
            }

            if self.dc.mtu_probing_complete_support() {
                MtuProbingCompleteSupport::Enabled.append_to_buffer(server_params);
            }
        }

        Ok(())
    }

    fn on_tls_context(&mut self, context: Box<dyn Any + Send>) {
        *self.tls_context = Some(context);
    }
}
