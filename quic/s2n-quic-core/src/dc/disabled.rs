// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    crypto::tls::TlsSession,
    dc::{ConnectionInfo, Endpoint, Path},
    stateless_reset::Token,
};

#[derive(Debug, Default)]
pub struct Disabled(());

impl Endpoint for Disabled {
    const ENABLED: bool = false;

    type Path = DisabledPath;

    fn new_path(&mut self, _connection_info: &ConnectionInfo) -> Self::Path {
        DisabledPath(())
    }
}

pub struct DisabledPath(());

impl Path for DisabledPath {
    fn on_path_secrets_ready(&mut self, _session: &impl TlsSession) {}

    fn on_peer_stateless_reset_tokens<'a>(
        &mut self,
        _stateless_reset_tokens: impl Iterator<Item = &'a Token>,
    ) {
    }

    fn stateless_reset_tokens(&mut self) -> &[Token] {
        &[]
    }
}
