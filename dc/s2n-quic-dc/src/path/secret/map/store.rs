// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{ApplicationData, ApplicationDataError, Entry};
use crate::{
    credentials::{Credentials, Id},
    packet::{secret_control as control, Packet, WireVersion},
    path::secret::{receiver, stateless_reset},
    psk::io::HandshakeReason,
};
use s2n_codec::EncoderBuffer;
use s2n_quic_core::{time, varint::VarInt};
use std::{net::SocketAddr, sync::Arc};
use tokio::task::JoinHandle;

/// Determines how the map delivers a control response (e.g. UnknownPathSecret)
/// when pre-authentication fails.
pub enum ControlResponse<'a> {
    /// The map sends the control packet directly via its background socket.
    SendDirect { peer: SocketAddr },
    /// The caller will handle delivery (e.g. via rate-limited UPS path).
    /// The encoded packet is written into the provided buffer.
    ReturnBuffer { out: &'a mut Vec<u8> },
}

pub trait Store: 'static + Send + Sync + time::Clock {
    fn secrets_len(&self) -> usize;

    fn peers_len(&self) -> usize;

    fn secrets_capacity(&self) -> usize;

    fn socket_sender_count(&self) -> usize;

    fn set_socket_sender_count(&self, count: usize);

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

    fn sign_flow_reset_packet(&self, packet: &control::FlowReset, out: &mut [u8]) -> Option<usize>;

    fn handle_flow_reset_packet<'a>(
        &self,
        packet: &'a control::flow_reset::Packet,
        peer: &SocketAddr,
    ) -> Option<&'a control::FlowReset>;

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
        control: ControlResponse<'_>,
    ) -> Option<Arc<Entry>> {
        let Some(state) = self.get_by_id_tracked(&identity.id) else {
            let packet = control::UnknownPathSecret {
                wire_version: WireVersion::ZERO,
                credential_id: identity.id,
                queue_id,
            };
            let stateless_reset = self.signer().sign(&identity.id);
            match control {
                ControlResponse::SendDirect { peer } => {
                    let mut buf = [0u8; control::MAX_PACKET_SIZE];
                    let len = packet.encode(EncoderBuffer::new(&mut buf), &stateless_reset);
                    self.send_control_packet(&peer, &mut buf[..len]);
                }
                ControlResponse::ReturnBuffer { out } => {
                    out.resize(control::UnknownPathSecret::MAX_PACKET_SIZE, 0);
                    let len = packet.encode(EncoderBuffer::new(out), &stateless_reset);
                    out.truncate(len);
                }
            }
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
