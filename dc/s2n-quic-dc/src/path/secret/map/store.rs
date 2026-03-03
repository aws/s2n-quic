// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{ApplicationData, ApplicationDataError, Entry};
use crate::{
    credentials::{Credentials, Id},
    packet::{secret_control as control, Packet, WireVersion},
    path::secret::{receiver, stateless_reset},
    psk::io::HandshakeReason,
};
use core::time::Duration;
use s2n_codec::EncoderBuffer;
use s2n_quic_core::varint::VarInt;
use std::{net::SocketAddr, sync::Arc};
use tokio::task::JoinHandle;

pub trait Store: 'static + Send + Sync {
    fn secrets_len(&self) -> usize;

    fn peers_len(&self) -> usize;

    fn secrets_capacity(&self) -> usize;

    fn should_evict_on_unknown_path_secret(&self) -> bool;

    fn drop_state(&self);

    fn on_new_path_secrets(&self, entry: Arc<Entry>);

    fn on_handshake_complete(&self, entry: Arc<Entry>);

    fn contains(&self, peer: &SocketAddr) -> bool;

    fn get_by_addr_untracked(&self, peer: &SocketAddr) -> Option<Arc<Entry>>;

    fn get_by_addr_tracked(&self, peer: &SocketAddr) -> Option<Arc<Entry>>;

    fn get_by_id_untracked(&self, id: &Id) -> Option<Arc<Entry>>;

    fn get_by_id_tracked(&self, id: &Id) -> Option<Arc<Entry>>;

    fn handle_unexpected_packet(&self, packet: &Packet, peer: &SocketAddr);

    fn handle_stale_key_packet<'a>(
        &self,
        packet: &'a control::stale_key::Packet,
        peer: &SocketAddr,
    ) -> Option<&'a control::StaleKey>;

    fn handle_replay_detected_packet<'a>(
        &self,
        packet: &'a control::replay_detected::Packet,
        peer: &SocketAddr,
    ) -> Option<&'a control::ReplayDetected>;

    fn handle_unknown_path_secret_packet<'a>(
        &self,
        packet: &'a control::unknown_path_secret::Packet,
        peer: &SocketAddr,
    ) -> Option<&'a control::UnknownPathSecret>;

    fn signer(&self) -> &stateless_reset::Signer;

    fn send_control_packet(&self, dst: &SocketAddr, buffer: &mut [u8]);

    fn rehandshake_period(&self) -> Duration;

    fn register_request_handshake(
        &self,
        cb: Box<dyn Fn(SocketAddr, HandshakeReason) -> Option<JoinHandle<()>> + Send + Sync>,
    );

    fn check_dedup(
        &self,
        entry: &Entry,
        key_id: VarInt,
        queue_id: Option<VarInt>,
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
    fn send_control_error(
        &self,
        entry: &Entry,
        credentials: &Credentials,
        queue_id: Option<VarInt>,
        error: receiver::Error,
    ) {
        let mut buffer = [0; control::MAX_PACKET_SIZE];
        let len = error
            .to_packet(entry, credentials, queue_id, &mut buffer)
            .len();
        let buffer = &mut buffer[..len];
        let dst = entry.peer();
        self.send_control_packet(dst, buffer);
    }

    #[inline]
    fn pre_authentication(
        &self,
        identity: &Credentials,
        queue_id: Option<VarInt>,
        control_out: &mut Vec<u8>,
    ) -> Option<Arc<Entry>> {
        let Some(state) = self.get_by_id_tracked(&identity.id) else {
            let packet = control::UnknownPathSecret {
                wire_version: WireVersion::ZERO,
                credential_id: identity.id,
                queue_id,
            };
            control_out.resize(control::UnknownPathSecret::MAX_PACKET_SIZE, 0);
            let stateless_reset = self.signer().sign(&identity.id);
            let encoder = EncoderBuffer::new(control_out);
            let len = packet.encode(encoder, &stateless_reset);
            control_out.truncate(len);
            return None;
        };

        match state.receiver().pre_authentication(identity) {
            Ok(()) => {}
            Err(e) => {
                self.send_control_error(&state, identity, queue_id, e);
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

    #[cfg(test)]
    fn reset_all_senders(&self);

    fn on_dc_connection_timeout(&self, peer_address: &SocketAddr);
}
