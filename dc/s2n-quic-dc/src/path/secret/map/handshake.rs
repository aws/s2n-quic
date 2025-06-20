// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{entry::ApplicationData, Entry, Map};
use crate::{
    packet::secret_control as control,
    path::secret::{receiver, schedule, sender},
};
use parking_lot::Mutex;
use s2n_quic_core::{
    dc::{self, ApplicationParams, DatagramInfo},
    endpoint, ensure, event,
};
use std::{error::Error, net::SocketAddr, sync::Arc};
use zeroize::Zeroizing;

const TLS_EXPORTER_LABEL: &str = "EXPERIMENTAL EXPORTER s2n-quic-dc";
const TLS_EXPORTER_CONTEXT: &str = "";
const TLS_EXPORTER_LENGTH: usize = schedule::EXPORT_SECRET_LEN;

#[derive(Clone)]
pub struct HandshakingPath {
    inner: Arc<Mutex<HandshakingPathInner>>,
}

struct HandshakingPathInner {
    peer: SocketAddr,
    dc_version: dc::Version,
    parameters: ApplicationParams,
    endpoint_type: s2n_quic_core::endpoint::Type,
    secret: Option<schedule::Secret>,
    entry: Option<Arc<Entry>>,
    application_data: Option<ApplicationData>,
    map: Map,

    error: Option<Box<dyn Error + Send + Sync>>,
}

impl HandshakingPath {
    fn new(connection_info: &dc::ConnectionInfo, map: Map) -> Self {
        let endpoint_type = match connection_info.endpoint_type {
            event::api::EndpointType::Server { .. } => endpoint::Type::Server,
            event::api::EndpointType::Client { .. } => endpoint::Type::Client,
        };

        HandshakingPath {
            inner: Arc::new(Mutex::new(HandshakingPathInner {
                peer: connection_info.remote_address.clone().into(),
                dc_version: connection_info.dc_version,
                parameters: connection_info.application_params.clone(),
                endpoint_type,
                secret: None,
                entry: None,
                application_data: None,
                map,
                error: None,
            })),
        }
    }

    pub fn entry(&self) -> Option<Arc<Entry>> {
        self.inner.lock().entry.clone()
    }

    pub fn take_error(&self) -> Option<Box<dyn Error + Send + Sync>> {
        self.inner.lock().error.take()
    }
}

impl dc::Endpoint for Map {
    type Path = HandshakingPath;

    fn new_path(&mut self, connection_info: &dc::ConnectionInfo) -> Option<Self::Path> {
        Some(HandshakingPath::new(connection_info, self.clone()))
    }

    fn on_possible_secret_control_packet(
        &mut self,
        // TODO: Maybe we should confirm that the sender IP at least matches the IP for the
        //       corresponding control secret?
        datagram_info: &DatagramInfo,
        payload: &mut [u8],
    ) -> bool {
        let payload = s2n_codec::DecoderBufferMut::new(payload);
        match control::Packet::decode(payload) {
            Ok((packet, tail)) => {
                // Probably a bug somewhere? There shouldn't be anything trailing in the buffer
                // after we decode a secret control packet.
                ensure!(tail.is_empty(), false);

                // If we successfully decoded a control packet, pass it into our map to handle.
                self.handle_control_packet(&packet, &datagram_info.remote_address.clone().into());

                true
            }
            Err(_) => false,
        }
    }
}

impl dc::Path for HandshakingPath {
    fn on_path_secrets_ready(
        &mut self,
        session: &impl s2n_quic_core::crypto::tls::TlsSession,
    ) -> Result<Vec<s2n_quic_core::stateless_reset::Token>, s2n_quic_core::transport::Error> {
        self.inner.lock().on_path_secrets_ready(session)
    }

    fn on_peer_stateless_reset_tokens<'a>(
        &mut self,
        stateless_reset_tokens: impl Iterator<Item = &'a s2n_quic_core::stateless_reset::Token>,
    ) {
        self.inner
            .lock()
            .on_peer_stateless_reset_tokens(stateless_reset_tokens)
    }

    fn on_dc_handshake_complete(&mut self) {
        self.inner.lock().on_dc_handshake_complete();
    }

    fn on_mtu_updated(&mut self, mtu: u16) {
        self.inner.lock().on_mtu_updated(mtu);
    }
}

impl HandshakingPathInner {
    fn on_path_secrets_ready(
        &mut self,
        session: &impl s2n_quic_core::crypto::tls::TlsSession,
    ) -> Result<Vec<s2n_quic_core::stateless_reset::Token>, s2n_quic_core::transport::Error> {
        match self.map.store.application_data(session) {
            Ok(application_data) => {
                self.application_data = application_data;
            }
            Err(err) => {
                self.error = Some(err.inner);
                return Err(s2n_quic_core::transport::Error::APPLICATION_ERROR.with_reason(err.msg));
            }
        };

        let mut material = Zeroizing::new([0; TLS_EXPORTER_LENGTH]);
        session
            .tls_exporter(
                TLS_EXPORTER_LABEL.as_bytes(),
                TLS_EXPORTER_CONTEXT.as_bytes(),
                &mut *material,
            )
            .map_err(|_| {
                s2n_quic_core::transport::Error::INTERNAL_ERROR.with_reason("tls exporter failed")
            })?;

        let cipher_suite = match session.cipher_suite() {
            s2n_quic_core::crypto::tls::CipherSuite::TLS_AES_128_GCM_SHA256 => {
                schedule::Ciphersuite::AES_GCM_128_SHA256
            }
            s2n_quic_core::crypto::tls::CipherSuite::TLS_AES_256_GCM_SHA384 => {
                schedule::Ciphersuite::AES_GCM_256_SHA384
            }
            _ => {
                return Err(s2n_quic_core::transport::Error::INTERNAL_ERROR
                    .with_reason("unsupported ciphersuite"))
            }
        };

        let secret =
            schedule::Secret::new(cipher_suite, self.dc_version, self.endpoint_type, &material);

        let stateless_reset = self.map.store.signer().sign(secret.id());
        self.secret = Some(secret);

        Ok(vec![stateless_reset.into()])
    }

    fn on_peer_stateless_reset_tokens<'a>(
        &mut self,
        stateless_reset_tokens: impl Iterator<Item = &'a s2n_quic_core::stateless_reset::Token>,
    ) {
        // TODO: support multiple stateless reset tokens
        let sender = sender::State::new(
            stateless_reset_tokens
                .into_iter()
                .next()
                .unwrap()
                .into_inner(),
        );

        let receiver = receiver::State::new();

        let entry = Entry::new(
            self.peer,
            self.secret
                .take()
                .expect("peer tokens are only received after secrets are ready"),
            sender,
            receiver,
            self.parameters.clone(),
            self.map.store.rehandshake_period(),
            self.application_data.take(),
        );
        let entry = Arc::new(entry);
        self.entry = Some(entry.clone());
        self.map.store.on_new_path_secrets(entry);
    }

    fn on_dc_handshake_complete(&mut self) {
        let entry = self.entry.clone().expect(
            "the dc handshake cannot be complete without \
        on_peer_stateless_reset_tokens creating a map entry",
        );
        self.map.store.on_handshake_complete(entry);
    }

    fn on_mtu_updated(&mut self, mtu: u16) {
        if let Some(entry) = self.entry.as_ref() {
            entry.update_max_datagram_size(mtu);
        }
    }
}
