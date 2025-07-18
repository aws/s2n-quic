// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{ApplicationData, ApplicationDataError, Entry};
use crate::{
    credentials::{Credentials, Id},
    packet::{secret_control as control, Packet, WireVersion},
    path::secret::{receiver, stateless_reset},
};
use core::time::Duration;
use s2n_codec::EncoderBuffer;
use std::{net::SocketAddr, sync::Arc};

pub trait Store: 'static + Send + Sync {
    fn secrets_len(&self) -> usize;

    fn peers_len(&self) -> usize;

    fn secrets_capacity(&self) -> usize;

    fn drop_state(&self);

    fn on_new_path_secrets(&self, entry: Arc<Entry>);

    fn on_handshake_complete(&self, entry: Arc<Entry>);

    fn contains(&self, peer: &SocketAddr) -> bool;

    fn get_by_addr_untracked(&self, peer: &SocketAddr) -> Option<Arc<Entry>>;

    fn get_by_addr_tracked(&self, peer: &SocketAddr) -> Option<Arc<Entry>>;

    fn get_by_id_untracked(&self, id: &Id) -> Option<Arc<Entry>>;

    fn get_by_id_tracked(&self, id: &Id) -> Option<Arc<Entry>>;

    fn handle_unexpected_packet(&self, packet: &Packet, peer: &SocketAddr);

    fn handle_control_packet(&self, packet: &control::Packet, peer: &SocketAddr);

    fn signer(&self) -> &stateless_reset::Signer;

    fn send_control_packet(&self, dst: &SocketAddr, buffer: &mut [u8]);

    fn rehandshake_period(&self) -> Duration;

    fn register_request_handshake(&self, cb: Box<dyn Fn(SocketAddr) + Send + Sync>);

    fn check_dedup(
        &self,
        entry: &Entry,
        key_id: s2n_quic_core::varint::VarInt,
    ) -> crate::crypto::open::Result;

    #[cfg(any(test, feature = "testing"))]
    fn test_insert(&self, entry: Arc<Entry>) {
        self.on_new_path_secrets(entry.clone());
        self.on_handshake_complete(entry);
    }

    /// Stops the cleaner thread
    #[cfg(test)]
    fn test_stop_cleaner(&self);

    #[inline]
    fn send_control_error(&self, entry: &Entry, credentials: &Credentials, error: receiver::Error) {
        let mut buffer = [0; control::MAX_PACKET_SIZE];
        let len = error.to_packet(entry, credentials, &mut buffer).len();
        let buffer = &mut buffer[..len];
        let dst = entry.peer();
        self.send_control_packet(dst, buffer);
    }

    #[inline]
    fn pre_authentication(
        &self,
        identity: &Credentials,
        control_out: &mut Vec<u8>,
    ) -> Option<Arc<Entry>> {
        let Some(state) = self.get_by_id_tracked(&identity.id) else {
            let packet = control::UnknownPathSecret {
                wire_version: WireVersion::ZERO,
                credential_id: identity.id,
            };
            control_out.resize(control::UnknownPathSecret::PACKET_SIZE, 0);
            let stateless_reset = self.signer().sign(&identity.id);
            let encoder = EncoderBuffer::new(control_out);
            packet.encode(encoder, &stateless_reset);
            return None;
        };

        match state.receiver().pre_authentication(identity) {
            Ok(()) => {}
            Err(e) => {
                self.send_control_error(&state, identity, e);
                control_out.resize(control::UnknownPathSecret::PACKET_SIZE, 0);

                return None;
            }
        }

        Some(state.clone())
    }

    #[allow(clippy::type_complexity)]
    fn register_make_application_data(
        &self,
        cb: Box<
            dyn Fn(
                    &dyn s2n_quic_core::crypto::tls::TlsSession,
                ) -> Result<Option<ApplicationData>, ApplicationDataError>
                + Send
                + Sync,
        >,
    );

    fn application_data(
        &self,
        session: &dyn s2n_quic_core::crypto::tls::TlsSession,
    ) -> Result<Option<ApplicationData>, ApplicationDataError>;
}
