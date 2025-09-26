// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    crypto::tls::TlsSession,
    dc,
    dc::{ApplicationParams, ConnectionInfo, DatagramInfo},
    stateless_reset, transport,
    varint::VarInt,
};
use core::{num::NonZeroU32, time::Duration};
use std::sync::{
    atomic::{AtomicU16, AtomicU8, Ordering},
    Arc,
};

pub struct MockDcEndpoint {
    stateless_reset_tokens: Vec<stateless_reset::Token>,
    pub on_possible_secret_control_packet_count: Arc<AtomicU8>,
    pub on_possible_secret_control_packet: fn() -> bool,
}

impl MockDcEndpoint {
    pub fn new(tokens: &[stateless_reset::Token]) -> Self {
        Self {
            stateless_reset_tokens: tokens.to_vec(),
            on_possible_secret_control_packet_count: Arc::new(AtomicU8::default()),
            on_possible_secret_control_packet: || false,
        }
    }
}

#[derive(Default)]
pub struct MockDcPath {
    pub on_path_secrets_ready_count: u8,
    pub on_peer_stateless_reset_tokens_count: u8,
    pub on_dc_handshake_complete: u8,
    pub stateless_reset_tokens: Vec<stateless_reset::Token>,
    pub peer_stateless_reset_tokens: Vec<stateless_reset::Token>,
    pub mtu: u16,
}

impl dc::Endpoint for MockDcEndpoint {
    type Path = MockDcPath;

    fn new_path(&mut self, connection_info: &ConnectionInfo) -> Option<Self::Path> {
        Some(MockDcPath {
            stateless_reset_tokens: self.stateless_reset_tokens.clone(),
            mtu: connection_info
                .application_params
                .max_datagram_size
                .load(Ordering::Relaxed),
            ..Default::default()
        })
    }

    fn on_possible_secret_control_packet(
        &mut self,
        _datagram_info: &DatagramInfo,
        _payload: &mut [u8],
    ) -> bool {
        self.on_possible_secret_control_packet_count
            .fetch_add(1, Ordering::Relaxed);
        (self.on_possible_secret_control_packet)()
    }
}

impl dc::Path for MockDcPath {
    fn on_path_secrets_ready(
        &mut self,
        _session: &impl TlsSession,
    ) -> Result<Vec<stateless_reset::Token>, transport::Error> {
        debug_assert_eq!(0, self.on_path_secrets_ready_count);
        self.on_path_secrets_ready_count += 1;
        Ok(self.stateless_reset_tokens.clone())
    }

    fn on_peer_stateless_reset_tokens<'a>(
        &mut self,
        stateless_reset_tokens: impl Iterator<Item = &'a stateless_reset::Token>,
    ) {
        debug_assert_eq!(0, self.on_peer_stateless_reset_tokens_count);
        self.on_peer_stateless_reset_tokens_count += 1;
        self.peer_stateless_reset_tokens
            .extend(stateless_reset_tokens);
    }

    fn on_dc_handshake_complete(&mut self) {
        debug_assert_eq!(0, self.on_dc_handshake_complete);
        self.on_dc_handshake_complete += 1;
    }

    fn on_mtu_updated(&mut self, mtu: u16) {
        self.mtu = mtu
    }
}

#[allow(clippy::declare_interior_mutable_const)]
pub const TEST_APPLICATION_PARAMS: ApplicationParams = ApplicationParams {
    max_datagram_size: AtomicU16::new(1472),
    remote_max_data: VarInt::from_u32(1472 * 10),
    local_send_max_data: VarInt::from_u32(1u32 << 25),
    local_recv_max_data: VarInt::from_u32(1u32 << 25),
    max_idle_timeout: NonZeroU32::new(Duration::from_secs(30).as_millis() as _),
};

pub const TEST_REHANDSHAKE_PERIOD: Duration = Duration::from_secs(3600 * 12);
