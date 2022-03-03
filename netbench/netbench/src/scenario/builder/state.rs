// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{
    certificate::{self, KeyPair},
    checkpoint::{Checkpoint, Park, Unpark},
    connection,
};
use crate::scenario::{Client, Server};
use core::fmt;
use std::{
    cell::{Cell, RefCell, RefMut},
    collections::HashMap,
    rc::Rc,
};

#[derive(Clone, Debug, Default)]
pub struct State {
    pub(crate) checkpoint: IdPool,
    pub(crate) servers: RefVec<Server>,
    pub(crate) clients: RefVec<Client>,
    pub(crate) trace: RefMap<String, u64>,
    pub(crate) certificates: RefVec<certificate::Certificate>,
    default_key_pair: Rc<Cell<Option<KeyPair>>>,
}

impl State {
    pub fn checkpoint<Endpoint, Location>(
        &self,
    ) -> (
        Checkpoint<Endpoint, Location, Park>,
        Checkpoint<Endpoint, Location, Unpark>,
    ) {
        let id = self.checkpoint.next_id();
        (Checkpoint::new(id), Checkpoint::new(id))
    }

    pub fn connection(&self) -> connection::State {
        connection::State::new(self.clone())
    }

    pub fn trace(&self, name: &str) -> u64 {
        if let Some(v) = self.trace.0.borrow().get(name) {
            return *v;
        }

        let mut map = self.trace.0.borrow_mut();

        let id = map.len() as u64;
        map.insert(name.to_string(), id);
        id
    }

    pub fn create_ca(&self) -> certificate::Authority {
        self.create_ca_with(|_| {})
    }

    pub fn create_ca_with<F: FnOnce(&mut certificate::AuthorityBuilder)>(
        &self,
        f: F,
    ) -> certificate::Authority {
        certificate::Authority::new(self.clone(), f)
    }

    pub fn default_key_pair(&self) -> KeyPair {
        if let Some(key) = self.default_key_pair.get() {
            key
        } else {
            let ca = self.create_ca();
            let pair = ca.key_pair();
            self.default_key_pair.set(Some(pair));
            pair
        }
    }
}

#[derive(Clone, Default)]
pub struct IdPool(Rc<RefCell<u64>>);

impl fmt::Debug for IdPool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "IdPool({})", *self.0.borrow())
    }
}

impl IdPool {
    pub fn next_id(&self) -> u64 {
        let mut next_id = self.0.borrow_mut();
        let id = *next_id;
        *next_id += 1;
        id
    }
}

#[derive(Clone, Debug)]
pub struct RefMap<Key, Value>(Rc<RefCell<HashMap<Key, Value>>>);

impl<Key, Value> Default for RefMap<Key, Value> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<Key: core::hash::Hash + Eq, Value> RefMap<Key, Value> {
    #[allow(dead_code)]
    pub fn insert(&self, key: Key, value: Value) {
        self.0.borrow_mut().insert(key, value);
    }

    pub fn take(&self) -> HashMap<Key, Value> {
        core::mem::take(&mut self.0.borrow_mut())
    }
}

#[derive(Clone, Debug)]
pub struct RefVec<Value>(Rc<RefCell<Vec<Value>>>);

impl<Value> Default for RefVec<Value> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<Value> RefVec<Value> {
    pub fn len(&self) -> usize {
        self.0.borrow().len()
    }

    pub fn push(&self, value: Value) -> usize {
        let mut v = self.0.borrow_mut();
        let len = v.len();
        v.push(value);
        len
    }

    pub fn take(&self) -> Vec<Value> {
        core::mem::take(&mut self.0.borrow_mut())
    }

    pub fn borrow_mut(&self) -> RefMut<Vec<Value>> {
        self.0.borrow_mut()
    }
}
