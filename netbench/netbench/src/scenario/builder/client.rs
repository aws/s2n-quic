// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{
    checkpoint::{Checkpoint, Park, Unpark},
    connection::{self, Connect, Connection},
    Local,
};
use crate::operation as op;
use core::marker::PhantomData;

#[derive(Debug)]
pub struct Builder {
    id: u64,
    state: super::State,
    ops: Vec<op::Client>,
}

impl Builder {
    pub(crate) fn new(id: u64, state: super::State) -> Self {
        Self {
            id,
            state,
            ops: vec![],
        }
    }

    pub fn connect_to<F: FnOnce(&mut connection::Builder<Client>), To: Connect<Client>>(
        &mut self,
        to: To,
        f: F,
    ) -> Connection<Client> {
        let mut builder = connection::Builder::new(self.state.connection());
        f(&mut builder);

        let template = builder.finish();
        let connection = Connection {
            endpoint_id: self.id,
            state: self.state.clone(),
            template,
            endpoint: PhantomData,
        };

        let op = to.connect_to(&connection);
        self.ops.push(op);

        connection
    }

    pub fn scope<F: FnOnce(&mut Scope)>(&mut self, f: F) -> &mut Self {
        let mut scope = Scope::new(self.id, self.state.clone());
        f(&mut scope);

        let threads = scope.finish();

        if threads.is_empty() {
            // no-op
        } else if threads.len() == 1 {
            // only a single thread was spawned, which is the same as not spawning it
            self.ops.extend(threads.into_iter().flatten());
        } else {
            self.ops.push(op::Client::Scope { threads });
        }

        self
    }

    pub fn checkpoint(
        &mut self,
    ) -> (
        Checkpoint<Client, Local, Park>,
        Checkpoint<Client, Local, Unpark>,
    ) {
        self.state.checkpoint()
    }

    pub(crate) fn finish(self) -> Vec<op::Client> {
        self.ops
    }
}

pub struct Scope {
    id: u64,
    state: super::State,
    threads: Vec<Vec<op::Client>>,
}

impl Scope {
    pub(crate) fn new(id: u64, state: super::State) -> Self {
        Self {
            id,
            state,
            threads: vec![],
        }
    }

    pub fn spawn<F: FnOnce(&mut Builder)>(&mut self, f: F) -> &mut Self {
        let mut builder = Builder::new(self.id, self.state.clone());
        f(&mut builder);
        self.threads.push(builder.finish());
        self
    }

    pub(crate) fn finish(self) -> Vec<Vec<op::Client>> {
        self.threads
    }
}

#[derive(Debug)]
pub struct Client {}
