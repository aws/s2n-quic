// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{
    checkpoint::{Checkpoint, Park, Unpark},
    state::{IdPool, RefVec},
    stream::{ReceiveStream, SendStream, Stream},
    Client, Endpoint, Local, Remote, Scope, Server,
};
use crate::operation as op;
use core::marker::PhantomData;
use std::sync::Arc;

#[derive(Clone, Debug, Default)]
pub struct State {
    peer_streams: RefVec<Vec<op::Connection>>,
    stream: IdPool,
    scenario: super::State,
}

impl State {
    pub(crate) fn new(scenario: super::State) -> Self {
        Self {
            scenario,
            ..Default::default()
        }
    }
}

impl core::ops::Deref for State {
    type Target = super::State;

    fn deref(&self) -> &Self::Target {
        &self.scenario
    }
}

#[derive(Debug)]
pub struct Builder<Endpoint> {
    ops: Vec<op::Connection>,
    state: State,
    endpoint: PhantomData<Endpoint>,
}

impl<E: Endpoint> Builder<E> {
    pub(crate) fn new(state: State) -> Self {
        Self {
            ops: vec![],
            state,
            endpoint: PhantomData,
        }
    }

    fn child_scope(&self) -> Self {
        Self::new(self.state.clone())
    }

    pub fn checkpoint<Location>(
        &self,
    ) -> (
        Checkpoint<E, Location, Park>,
        Checkpoint<E, Location, Unpark>,
    ) {
        self.state.checkpoint()
    }

    sync!(E, Local);
    sleep!();
    trace!();
    iterate!();

    pub fn scope<F: FnOnce(&mut Scope<E>)>(&mut self, f: F) -> &mut Self {
        let mut scope = Scope::new(self.state.clone());
        f(&mut scope);

        let threads = scope.finish();

        if threads.is_empty() {
            // no-op
        } else if threads.len() == 1 {
            // only a single thread was spawned, which is the same as not spawning it
            self.ops.extend(threads.into_iter().flatten());
        } else {
            self.ops.push(op::Connection::Scope { threads });
        }

        self
    }

    pub fn concurrently<A: FnOnce(&mut Builder<E>), B: FnOnce(&mut Builder<E>)>(
        &mut self,
        a: A,
        b: B,
    ) -> &mut Self {
        self.scope(|scope| {
            scope.spawn(a);
            scope.spawn(b);
        })
    }

    pub fn open_bidirectional_stream<
        L: FnOnce(&mut Stream<E, Local>),
        R: FnOnce(&mut Stream<E::Peer, Remote>),
    >(
        &mut self,
        local: L,
        remote: R,
    ) -> &mut Self {
        let id = self.state.stream.next_id();
        let mut local_stream = Stream::new(id, self.state.clone());
        let mut remote_stream = Stream::new(id, self.state.clone());

        local(&mut local_stream);
        remote(&mut remote_stream);

        self.ops
            .push(op::Connection::OpenBidirectionalStream { stream_id: id });
        self.ops.extend(local_stream.finish());

        debug_assert_eq!(id, self.state.peer_streams.len() as u64);
        self.state.peer_streams.push(remote_stream.finish());

        self
    }

    pub fn open_send_stream<
        L: FnOnce(&mut SendStream<E, Local>),
        R: FnOnce(&mut ReceiveStream<E::Peer, Remote>),
    >(
        &mut self,
        local: L,
        remote: R,
    ) -> &mut Self {
        let id = self.state.stream.next_id();
        let mut local_stream = SendStream::new(id, self.state.clone());
        let mut remote_stream = ReceiveStream::new(id, self.state.clone());

        local(&mut local_stream);
        remote(&mut remote_stream);

        self.ops
            .push(op::Connection::OpenSendStream { stream_id: id });
        self.ops.extend(local_stream.finish());

        debug_assert_eq!(id, self.state.peer_streams.len() as u64);
        self.state.peer_streams.push(remote_stream.finish());

        self
    }

    pub(crate) fn finish(self) -> crate::scenario::Connection {
        let peer_streams = self.state.peer_streams.take();
        let ops = self.ops;
        crate::scenario::Connection { ops, peer_streams }
    }

    pub(crate) fn finish_scope(self) -> Vec<op::Connection> {
        self.ops
    }
}

#[derive(Debug)]
pub struct Connection<Endpoint> {
    pub(crate) state: super::State,
    pub(crate) endpoint_id: u64,
    pub(crate) template: crate::scenario::Connection,
    pub(crate) endpoint: PhantomData<Endpoint>,
}

pub trait Connect<Endpoint> {
    fn connect_to(&self, handle: &Connection<Endpoint>) -> op::Client;
}

impl Connect<Client> for Connection<Server> {
    fn connect_to(&self, handle: &Connection<Client>) -> op::Client {
        let server_id = self.endpoint_id;
        let server = &mut self.state.servers.borrow_mut()[server_id as usize];

        fn push(
            connections: &mut Vec<Arc<crate::scenario::Connection>>,
            ops: &Vec<op::Connection>,
            peer_streams: &Vec<Vec<op::Connection>>,
        ) -> u64 {
            // try to dedupe the connection operations if one exists
            for (id, prev) in connections.iter().enumerate() {
                if &prev.ops == ops && &prev.peer_streams == peer_streams {
                    return id as u64;
                }
            }

            let id = connections.len() as u64;

            connections.push(Arc::new(crate::scenario::Connection {
                ops: ops.clone(),
                peer_streams: peer_streams.clone(),
            }));

            id
        }

        let server_connection_id = push(
            &mut server.connections,
            &self.template.ops,
            &handle.template.peer_streams,
        );

        let certificate_authority = server.certificate_authority;

        let client = &mut self.state.clients.borrow_mut()[handle.endpoint_id as usize];
        let client_connection_id = push(
            &mut client.connections,
            &handle.template.ops,
            &self.template.peer_streams,
        );

        if !client
            .certificate_authorities
            .contains(&certificate_authority)
        {
            client.certificate_authorities.push(certificate_authority);
        }

        op::Client::Connect {
            server_id,
            router_id: None,
            server_connection_id,
            client_connection_id,
        }
    }
}

impl Connect<Client> for Server {
    fn connect_to(&self, handle: &Connection<Client>) -> op::Client {
        self.with(|_| {
            // empty instructions
        })
        .connect_to(handle)
    }
}

impl Connect<Client> for &Server {
    fn connect_to(&self, handle: &Connection<Client>) -> op::Client {
        self.with(|_| {
            // empty instructions
        })
        .connect_to(handle)
    }
}
