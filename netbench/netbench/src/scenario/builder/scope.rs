// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{connection, Endpoint};
use crate::operation as op;
use core::marker::PhantomData;

pub struct Scope<Endpoint> {
    state: connection::State,
    threads: Vec<Vec<op::Connection>>,
    endpoint: PhantomData<Endpoint>,
}

impl<E: Endpoint> Scope<E> {
    pub(crate) fn new(state: connection::State) -> Self {
        Self {
            state,
            threads: vec![],
            endpoint: PhantomData,
        }
    }

    pub fn spawn<F: FnOnce(&mut connection::Builder<E>)>(&mut self, f: F) -> &mut Self {
        let mut builder = connection::Builder::new(self.state.clone());
        f(&mut builder);
        self.threads.push(builder.finish_scope());
        self
    }

    pub(crate) fn finish(self) -> Vec<Vec<op::Connection>> {
        self.threads
    }
}
