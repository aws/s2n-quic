// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{
    checkpoint::{Checkpoint, Park, Unpark},
    connection::{self, Connect, Connection},
    Local,
};
use crate::scenario::ClientOperation;
use core::marker::PhantomData;

#[derive(Debug)]
pub struct Builder {
    id: u64,
    state: super::State,
    ops: Vec<ClientOperation>,
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

    pub fn checkpoint(
        &mut self,
    ) -> (
        Checkpoint<Client, Local, Park>,
        Checkpoint<Client, Local, Unpark>,
    ) {
        self.state.checkpoint()
    }

    pub(crate) fn finish(self) -> Vec<ClientOperation> {
        self.ops
    }
}

#[derive(Debug)]
pub struct Client {}
