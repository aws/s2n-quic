// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{
    certificate::KeyPair,
    connection::{self, Connection},
};
use core::marker::PhantomData;

#[derive(Debug)]
pub struct Server {
    id: u64,
    state: super::State,
}

pub struct Builder {
    state: super::State,
    key_pair: Option<KeyPair>,
}

impl Builder {
    pub fn set_cert(&mut self, key_pair: KeyPair) -> &mut Self {
        self.key_pair = Some(key_pair);
        self
    }
}

impl Server {
    pub(crate) fn new<F: FnOnce(&mut Builder)>(state: super::State, f: F) -> Self {
        let mut builder = Builder {
            state,
            key_pair: None,
        };
        f(&mut builder);
        let state = builder.state;

        let mut server = crate::scenario::Server::default();

        let key_pair = builder.key_pair.unwrap_or_else(|| state.default_key_pair());
        server.certificate = key_pair.certificate;
        server.private_key = key_pair.private_key;
        server.certificate_authority = key_pair.authority;

        let id = state.servers.push(server) as u64;

        Self { id, state }
    }

    pub(crate) fn with<F: FnOnce(&mut connection::Builder<Server>)>(
        &self,
        f: F,
    ) -> Connection<Server> {
        let mut builder = connection::Builder::new(self.state.connection());
        f(&mut builder);

        Connection {
            endpoint_id: self.id,
            state: self.state.clone(),
            template: builder.finish(),
            endpoint: PhantomData,
        }
    }
}
