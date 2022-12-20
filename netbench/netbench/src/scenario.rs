// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{operation as op, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path, sync::Arc};

pub mod builder;
mod id;

pub use builder::Builder;
pub use id::Id;

#[derive(Clone, Debug, Default, Deserialize, Serialize, Hash)]
pub struct Scenario {
    pub id: Id,
    pub clients: Vec<Arc<Client>>,
    pub servers: Vec<Arc<Server>>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub routers: Vec<Arc<Router>>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub traces: Arc<Vec<String>>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub certificates: Vec<Arc<Certificate>>,
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

    pub fn write<W: std::io::Write>(&self, out: &mut W) -> std::io::Result<()> {
        serde_json::to_writer(out, self)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Hash)]
pub struct Client {
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub name: String,
    pub scenario: Vec<op::Client>,
    pub connections: Vec<Arc<Connection>>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub configuration: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub certificate_authorities: Vec<u64>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Hash)]
pub struct Server {
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub name: String,
    pub connections: Vec<Arc<Connection>>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub configuration: BTreeMap<String, String>,
    pub private_key: u64,
    pub certificate: u64,
    pub certificate_authority: u64,
}

impl Server {
    pub fn on_server_name(&self, server_name: &str) -> Result<&Arc<Connection>> {
        let (conn_idx, _) = server_name.split_once('.').ok_or("invalid hostname")?;
        let conn_idx: usize = conn_idx.parse()?;
        let conn = self.connections.get(conn_idx).ok_or("invalid connection")?;
        Ok(conn)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Hash)]
pub struct Connection {
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub ops: Vec<op::Connection>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub peer_streams: Vec<Vec<op::Connection>>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Hash)]
pub struct Router {
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub name: String,
    pub scenario: Vec<op::Router>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub configuration: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Hash)]
pub struct Certificate {
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub pem: String,

    #[serde(skip_serializing_if = "Vec::is_empty", with = "pkcs12_format", default)]
    pub pkcs12: Vec<u8>,
}

pub(crate) mod pkcs12_format {
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if serializer.is_human_readable() {
            let out = base64::encode(bytes);
            serializer.serialize_str(&out)
        } else {
            serializer.serialize_bytes(bytes)
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        if deserializer.is_human_readable() {
            let s = String::deserialize(deserializer)?;
            let out = base64::decode(s).map_err(serde::de::Error::custom)?;
            Ok(out)
        } else {
            Vec::deserialize(deserializer)
        }
    }
}
