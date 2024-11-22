// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::Entry;
use crate::{
    credentials::{Credentials, Id},
    fixed_map::ReadGuard,
    packet::{secret_control as control, Packet, WireVersion},
    path::secret::{receiver, stateless_reset, HandshakeKind},
};
use core::time::Duration;
use s2n_codec::EncoderBuffer;
use std::{net::SocketAddr, sync::Arc};

pub trait Store: 'static + Send + Sync {
    fn secrets_len(&self) -> usize;

    fn peers_len(&self) -> usize;

    fn secrets_capacity(&self) -> usize;

    fn drop_state(&self);

    fn contains(&self, peer: SocketAddr) -> bool;

    fn on_new_path_secrets(&self, entry: Arc<Entry>);

    fn on_handshake_complete(&self, entry: Arc<Entry>);

    #[cfg(any(test, feature = "testing"))]
    fn test_insert(&self, entry: Arc<Entry>) {
        self.on_new_path_secrets(entry.clone());
        self.on_handshake_complete(entry);
    }

    fn get_by_addr_tracked(
        &self,
        peer: &SocketAddr,
        handshake: HandshakeKind,
    ) -> Option<ReadGuard<Arc<Entry>>>;

    fn get_by_id_untracked(&self, id: &Id) -> Option<ReadGuard<Arc<Entry>>>;

    fn get_by_id_tracked(&self, id: &Id) -> Option<ReadGuard<Arc<Entry>>>;

    fn handle_unexpected_packet(&self, packet: &Packet, peer: &SocketAddr);

    fn handle_control_packet(&self, packet: &control::Packet, peer: &SocketAddr);

    fn signer(&self) -> &stateless_reset::Signer;

    fn receiver(&self) -> &Arc<receiver::Shared>;

    fn send_control_packet(&self, dst: &SocketAddr, buffer: &mut [u8]);

    fn rehandshake_period(&self) -> Duration;

    fn check_dedup(
        &self,
        entry: &Entry,
        key_id: s2n_quic_core::varint::VarInt,
    ) -> crate::crypto::open::Result;

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
}
