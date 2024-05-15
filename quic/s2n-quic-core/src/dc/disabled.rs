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

    type Path = ();

    fn new_path(&mut self, _connection_info: &ConnectionInfo) -> Option<Self::Path> {
        None
    }
}

// The Disabled Endpoint returns `None`, so this is not used
impl Path for () {
    fn on_path_secrets_ready(&mut self, _session: &impl TlsSession) {
        unimplemented!()
    }

    fn on_peer_stateless_reset_tokens<'a>(
        &mut self,
        _stateless_reset_tokens: impl Iterator<Item = &'a Token>,
    ) {
        unimplemented!()
    }

    fn stateless_reset_tokens(&mut self) -> &[Token] {
        unimplemented!()
    }
}
