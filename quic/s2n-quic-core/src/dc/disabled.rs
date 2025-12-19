// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    crypto::tls::TlsSession,
    dc::{ConnectionInfo, DatagramInfo, Endpoint, Path},
    stateless_reset, transport,
};
use alloc::vec::Vec;

#[derive(Debug, Default)]
pub struct Disabled(());

impl Endpoint for Disabled {
    const ENABLED: bool = false;

    type Path = ();

    fn new_path(&mut self, _connection_info: &ConnectionInfo) -> Option<Self::Path> {
        None
    }

    fn on_possible_secret_control_packet(
        &mut self,
        _datagram_info: &DatagramInfo,
        _payload: &mut [u8],
    ) -> bool {
        unreachable!()
    }

    fn mtu_probing_complete_support(&self) -> bool {
        false
    }
}

// The Disabled Endpoint returns `None`, so this is not used
impl Path for () {
    fn on_path_secrets_ready(
        &mut self,
        _session: &impl TlsSession,
    ) -> Result<Vec<stateless_reset::Token>, transport::Error> {
        unimplemented!()
    }

    fn on_peer_stateless_reset_tokens<'a>(
        &mut self,
        _stateless_reset_tokens: impl Iterator<Item = &'a stateless_reset::Token>,
    ) {
        unimplemented!()
    }

    fn on_dc_handshake_complete(&mut self) {
        unimplemented!()
    }

    fn on_mtu_updated(&mut self, _mtu: u16) {
        unimplemented!()
    }
}
