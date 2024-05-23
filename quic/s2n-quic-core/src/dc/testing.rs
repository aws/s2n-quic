// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{crypto::tls::TlsSession, dc, dc::ConnectionInfo, stateless_reset};

pub struct MockDcEndpoint {
    stateless_reset_tokens: Vec<stateless_reset::Token>,
}

impl MockDcEndpoint {
    pub fn new(tokens: &[stateless_reset::Token]) -> Self {
        Self {
            stateless_reset_tokens: tokens.to_vec(),
        }
    }
}

#[derive(Default)]
pub struct MockDcPath {
    pub on_path_secrets_ready_count: u8,
    pub on_peer_stateless_reset_tokens_count: u8,
    pub stateless_reset_tokens: Vec<stateless_reset::Token>,
    pub peer_stateless_reset_tokens: Vec<stateless_reset::Token>,
}

impl dc::Endpoint for MockDcEndpoint {
    type Path = MockDcPath;

    fn new_path(&mut self, _connection_info: &ConnectionInfo) -> Option<Self::Path> {
        Some(MockDcPath {
            stateless_reset_tokens: self.stateless_reset_tokens.clone(),
            ..Default::default()
        })
    }
}

impl dc::Path for MockDcPath {
    fn on_path_secrets_ready(&mut self, _session: &impl TlsSession) -> Vec<stateless_reset::Token> {
        self.on_path_secrets_ready_count += 1;
        self.stateless_reset_tokens.clone()
    }

    fn on_peer_stateless_reset_tokens<'a>(
        &mut self,
        stateless_reset_tokens: impl Iterator<Item = &'a stateless_reset::Token>,
    ) {
        self.on_peer_stateless_reset_tokens_count += 1;
        self.peer_stateless_reset_tokens
            .extend(stateless_reset_tokens);
    }
}
