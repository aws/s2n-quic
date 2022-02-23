// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

macro_rules! sleep {
    () => {
        pub fn sleep(&mut self, amount: core::time::Duration) -> &mut Self {
            self.ops
                .push(crate::operation::Connection::Sleep { amount });
            self
        }
    };
}

macro_rules! trace {
    () => {
        pub fn trace(&mut self, name: &str) -> &mut Self {
            let trace_id = self.state.trace(name);
            self.ops
                .push(crate::operation::Connection::Trace { trace_id });
            self
        }
    };
}

#[macro_use]
pub mod checkpoint;

pub mod client;
pub mod connection;
pub mod scope;
pub mod server;
mod state;
pub mod stream;

pub use client::Client;
pub use connection::Connection;
pub use scope::Scope;
pub use server::Server;
pub use stream::Stream;

use state::State;

#[derive(Debug)]
pub struct Builder {
    state: State,
}

impl Builder {
    pub(super) fn new() -> Self {
        Self {
            state: Default::default(),
        }
    }

    pub fn create_server(&mut self) -> Server {
        Server::new(self.state.clone())
    }

    pub fn create_client<F: FnOnce(&mut client::Builder)>(&mut self, f: F) {
        let id = self.state.clients.push(super::Client {
            name: String::new(),
            scenario: vec![],
            connections: vec![],
            configuration: Default::default(),
        }) as u64;

        let mut builder = client::Builder::new(id, self.state.clone());
        f(&mut builder);

        self.state.clients.borrow_mut()[id as usize].scenario = builder.finish();
    }

    pub(super) fn finish(self) -> super::Scenario {
        let clients = self.state.clients.take();
        let servers = self.state.servers.take();
        let mut traces = self.state.trace.take().into_iter().collect::<Vec<_>>();
        traces.sort_by(|(_, a), (_, b)| a.cmp(b));
        let traces = traces.into_iter().map(|(value, _)| value).collect();

        let mut scenario = super::Scenario {
            id: Default::default(),
            clients,
            servers,
            // TODO implement router builder
            routers: vec![],
            traces,
        };

        let mut hash = crate::scenario::Id::hasher();
        core::hash::Hash::hash(&scenario, &mut hash);
        scenario.id = hash.finish();

        scenario
    }
}

pub trait Endpoint {
    type Peer: Endpoint;
}

impl Endpoint for Client {
    type Peer = Server;
}

impl Endpoint for Server {
    type Peer = Client;
}

#[derive(Debug)]
pub struct Local;

#[derive(Debug)]
pub struct Remote;

#[cfg(test)]
mod tests;
