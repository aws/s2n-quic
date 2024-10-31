// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{Entry, Map};
use crate::{
    packet::secret_control as control,
    path::secret::{schedule, sender},
};
use s2n_quic_core::{
    dc::{self, ApplicationParams, DatagramInfo},
    endpoint, ensure, event,
};
use std::{net::SocketAddr, sync::Arc};
use zeroize::Zeroizing;

const TLS_EXPORTER_LABEL: &str = "EXPERIMENTAL EXPORTER s2n-quic-dc";
const TLS_EXPORTER_CONTEXT: &str = "";
const TLS_EXPORTER_LENGTH: usize = schedule::EXPORT_SECRET_LEN;

pub struct HandshakingPath {
    peer: SocketAddr,
    dc_version: dc::Version,
    parameters: ApplicationParams,
    endpoint_type: s2n_quic_core::endpoint::Type,
    secret: Option<schedule::Secret>,
    entry: Option<Arc<Entry>>,
    map: Map,
}

impl HandshakingPath {
    fn new(connection_info: &dc::ConnectionInfo, map: Map) -> Self {
        let endpoint_type = match connection_info.endpoint_type {
            event::api::EndpointType::Server { .. } => endpoint::Type::Server,
            event::api::EndpointType::Client { .. } => endpoint::Type::Client,
        };

        Self {
            peer: connection_info.remote_address.clone().into(),
            dc_version: connection_info.dc_version,
            parameters: connection_info.application_params.clone(),
            endpoint_type,
            secret: None,
            entry: None,
            map,
        }
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
        _datagram_info: &DatagramInfo,
        payload: &mut [u8],
    ) -> bool {
        let payload = s2n_codec::DecoderBufferMut::new(payload);
        match control::Packet::decode(payload) {
            Ok((packet, tail)) => {
                // Probably a bug somewhere? There shouldn't be anything trailing in the buffer
                // after we decode a secret control packet.
                ensure!(tail.is_empty(), false);

                // If we successfully decoded a control packet, pass it into our map to handle.
                self.handle_control_packet(&packet);

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
        let mut material = Zeroizing::new([0; TLS_EXPORTER_LENGTH]);
        session
            .tls_exporter(
                TLS_EXPORTER_LABEL.as_bytes(),
                TLS_EXPORTER_CONTEXT.as_bytes(),
                &mut *material,
            )
            .unwrap();

        let cipher_suite = match session.cipher_suite() {
            s2n_quic_core::crypto::tls::CipherSuite::TLS_AES_128_GCM_SHA256 => {
                schedule::Ciphersuite::AES_GCM_128_SHA256
            }
            s2n_quic_core::crypto::tls::CipherSuite::TLS_AES_256_GCM_SHA384 => {
                schedule::Ciphersuite::AES_GCM_256_SHA384
            }
            _ => return Err(s2n_quic_core::transport::Error::INTERNAL_ERROR),
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

        let receiver = self.map.store.receiver().clone().new_receiver();

        let entry = Entry::new(
            self.peer,
            self.secret
                .take()
                .expect("peer tokens are only received after secrets are ready"),
            sender,
            receiver,
            self.parameters.clone(),
            self.map.store.rehandshake_period(),
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
