// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::units::duration_format;
use core::time::Duration;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, time::SystemTime};

pub trait Print: Serialize {
    fn print(&self) -> crate::Result<()> {
        use std::io::Write;
        let out = std::io::stdout();
        let mut out = out.lock();
        serde_json::to_writer(&mut out, self)?;
        writeln!(out)?;
        Ok(())
    }
}

impl<T: Serialize> Print for T {}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Initialize {
    pub pid: u64,
    pub driver: String,
    pub scenario: String,
    pub start_time: SystemTime,
    #[serde(default, skip_serializing_if = "is_default")]
    pub traces: Vec<String>,
}

impl Default for Initialize {
    fn default() -> Self {
        Self {
            pid: 0,
            driver: Default::default(),
            scenario: Default::default(),
            start_time: std::time::SystemTime::now(),
            traces: vec![],
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Stats {
    #[serde(rename = "t", with = "duration_format")]
    pub time: Duration,
    #[serde(default, skip_serializing_if = "is_default")]
    pub cpu: f32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub cycles: u64,
    #[serde(default, skip_serializing_if = "is_default")]
    pub instructions: u64,
    #[serde(default, skip_serializing_if = "is_default")]
    pub branches: u64,
    #[serde(default, skip_serializing_if = "is_default")]
    pub context_switches: u64,
    #[serde(default, skip_serializing_if = "is_default")]
    pub memory: u64,
    #[serde(default, skip_serializing_if = "is_default")]
    pub virtual_memory: u64,
    #[serde(default, skip_serializing_if = "is_default")]
    pub syscalls: u64,
    #[serde(default, skip_serializing_if = "is_default")]
    pub connections: u64,
    #[serde(default, skip_serializing_if = "is_default")]
    pub accept: u64,
    #[serde(default, skip_serializing_if = "is_default")]
    pub allocs: Stat,
    #[serde(default, skip_serializing_if = "is_default")]
    pub reallocs: Stat,
    #[serde(default, skip_serializing_if = "is_default")]
    pub deallocs: Stat,
    #[serde(default, skip_serializing_if = "is_default")]
    pub connect_time: Stat,
    #[serde(default, skip_serializing_if = "is_default")]
    pub send: HashMap<StreamId, Stat>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub receive: HashMap<StreamId, Stat>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub profiles: HashMap<u64, Histogram>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stat {
    #[serde(default, skip_serializing_if = "is_default")]
    pub count: u64,
    #[serde(default, skip_serializing_if = "is_default")]
    pub total: u64,
}

impl Stat {
    pub fn average(&self) -> f64 {
        self.total as f64 / self.count as f64
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Histogram {
    #[serde(default, skip_serializing_if = "is_default")]
    pub stat: Stat,
    #[serde(default, skip_serializing_if = "is_default")]
    pub buckets: Vec<Bucket>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bucket {
    pub lower: u64,
    pub upper: u64,
    pub count: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StreamId {
    pub connection_id: u64,
    pub id: u64,
}

impl<'de> Deserialize<'de> for StreamId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let (connection_id, id) = s.split_once(':').unwrap();
        let connection_id = connection_id.parse().unwrap();
        let id = id.parse().unwrap();
        Ok(Self { connection_id, id })
    }
}

impl Serialize for StreamId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use std::io::{Cursor, Write};

        let mut out = [0u8; 1024];
        let mut f = Cursor::new(&mut out[..]);
        write!(f, "{}:{}", self.connection_id, self.id).unwrap();
        let len = f.position() as usize;

        let out = &out[..len];
        let out = unsafe { core::str::from_utf8_unchecked(out) };

        serializer.serialize_str(out)
    }
}

#[inline(always)]
fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    &T::default() == v
}
