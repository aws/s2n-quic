// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    operation::{ClientOperation, ConnectionOperation, RouterOperation},
    Result,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

pub mod builder;
mod id;

pub use builder::Builder;
pub use id::Id;

#[derive(Clone, Debug, Default, Deserialize, Serialize, Hash)]
pub struct Scenario {
    pub id: Id,
    pub clients: Vec<Client>,
    pub servers: Vec<Server>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub routers: Vec<Router>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub traces: Vec<String>,
}

impl Scenario {
    pub fn build<F: FnOnce(&mut builder::Builder)>(f: F) -> Self {
        let mut builder = builder::Builder::new();
        f(&mut builder);
        builder.finish()
    }

    pub fn open(path: &Path) -> Result<Self> {
        let file = std::fs::File::open(path)?;
        let mut file = std::io::BufReader::new(file);
        let scenario = serde_json::from_reader(&mut file)?;
        Ok(scenario)
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Hash)]
pub struct Client {
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub name: String,
    pub scenario: Vec<ClientOperation>,
    pub connections: Vec<Connection>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub configuration: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Hash)]
pub struct Server {
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub name: String,
    pub connections: Vec<Connection>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub configuration: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Hash)]
pub struct Connection {
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub ops: Vec<ConnectionOperation>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub peer_streams: Vec<Vec<ConnectionOperation>>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Hash)]
pub struct Router {
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub name: String,
    pub scenario: Vec<RouterOperation>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub configuration: BTreeMap<String, String>,
}
