// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::connection::{self, Connection};
use core::marker::PhantomData;

#[derive(Debug)]
pub struct Server {
    id: u64,
    state: super::State,
}

impl Server {
    pub(crate) fn new(state: super::State) -> Self {
        let id = state.servers.push(crate::scenario::Server::default()) as u64;

        Self { id, state }
    }

    pub fn with<F: FnOnce(&mut connection::Builder<Server>)>(&self, f: F) -> Connection<Server> {
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
