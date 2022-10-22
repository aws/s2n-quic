// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;

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

        pub fn profile<F: FnOnce(&mut Self)>(&mut self, name: &str, f: F) -> &mut Self {
            let trace_id = self.state.trace(name);
            let mut builder = self.child_scope();
            f(&mut builder);
            let operations = builder.finish_scope();

            self.ops.push(crate::operation::Connection::Profile {
                trace_id,
                operations,
            });

            self
        }
    };
}

macro_rules! iterate {
    () => {
        pub fn iterate<I: Into<crate::operation::IterateValue>, F: FnOnce(&mut Self)>(
            &mut self,
            count: I,
            f: F,
        ) -> &mut Self {
            let mut builder = self.child_scope();
            f(&mut builder);
            let mut operations = builder.finish_scope();

            let count = count.into();

            if operations.is_empty() || count.is_zero() {
                return self;
            }

            let mut trace_id = None;

            // optimize out nested iterate/profile statements
            if operations.len() == 1 {
                if let Some(crate::operation::Connection::Profile {
                    trace_id: child_id,
                    operations: child,
                }) = operations.get_mut(0)
                {
                    trace_id = Some(*child_id);
                    operations = core::mem::take(child);
                }
            }

            self.ops.push(crate::operation::Connection::Iterate {
                value: count,
                operations,
                trace_id,
            });

            self
        }
    };
}

#[macro_use]
pub mod checkpoint;

pub mod certificate;
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

    pub fn create_ca(&mut self) -> certificate::Authority {
        self.create_ca_with(|_| {})
    }

    pub fn create_ca_with<F: FnOnce(&mut certificate::AuthorityBuilder)>(
        &mut self,
        f: F,
    ) -> certificate::Authority {
        self.state.create_ca_with(f)
    }

    pub fn create_server(&mut self) -> Server {
        self.create_server_with(|_| {})
    }

    pub fn create_server_with<F: FnOnce(&mut server::Builder)>(&mut self, f: F) -> Server {
        Server::new(self.state.clone(), f)
    }

    pub fn create_client<F: FnOnce(&mut client::Builder)>(&mut self, f: F) {
        let id = self.state.clients.push(super::Client {
            name: String::new(),
            scenario: vec![],
            connections: vec![],
            configuration: Default::default(),
            certificate_authorities: vec![],
        }) as u64;

        let mut builder = client::Builder::new(id, self.state.clone());
        f(&mut builder);

        let client = &mut self.state.clients.borrow_mut()[id as usize];

        client.scenario = builder.finish();
    }

    pub(super) fn finish(self) -> super::Scenario {
        let clients = self
            .state
            .clients
            .take()
            .into_iter()
            .map(|mut client| {
                client.certificate_authorities.sort_unstable();

                Arc::new(client)
            })
            .collect();
        let servers = self
            .state
            .servers
            .take()
            .into_iter()
            .map(Arc::new)
            .collect();
        let mut traces = self.state.trace.take().into_iter().collect::<Vec<_>>();
        traces.sort_by(|(_, a), (_, b)| a.cmp(b));
        let traces = Arc::new(traces.into_iter().map(|(value, _)| value).collect());
        let certificates = self.state.certificates.take();

        let mut scenario = super::Scenario {
            id: Default::default(),
            clients,
            servers,
            // TODO implement router builder
            routers: vec![],
            traces,
            certificates: vec![],
        };

        let mut hash = crate::scenario::Id::hasher();
        core::hash::Hash::hash(&scenario, &mut hash);
        core::hash::Hash::hash(&certificates, &mut hash);
        scenario.id = hash.finish();

        scenario.certificates = certificate::Certificate::build_all(certificates, &scenario.id);

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
