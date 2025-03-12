// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::fmt;
use once_cell::sync::Lazy;
use prost::Message;
use s2n_quic::provider::event::{events, Timestamp};
use std::{
    cell::RefCell,
    convert::TryInto,
    io::{self, Read},
    str::FromStr,
};

thread_local! {
    static BUFFER: RefCell<Vec<u8>> = Default::default();
}

#[allow(clippy::large_enum_variant)] // the small variants are only used once so it's not a big deal
#[derive(Debug)]
pub enum Stats {
    Setup(Setup),
    Parameters(Parameters),
    Connection(Connection),
}

impl Stats {
    pub fn reader<R: io::Read>(read: R) -> impl Iterator<Item = io::Result<Self>> {
        Reader {
            read: io::BufReader::new(read),
        }
    }

    fn write<W: io::Write, M: Message>(mut w: W, tag: u8, msg: &M) -> io::Result<()> {
        BUFFER.with(|cell| {
            let mut buffer = cell.borrow_mut();
            let buffer = &mut *buffer;
            buffer.clear();

            buffer.push(tag);
            let len = msg.encoded_len();
            let len: u16 = len.try_into().unwrap();
            buffer.extend_from_slice(&len.to_le_bytes());
            msg.encode(buffer).unwrap();

            w.write_all(buffer)?;

            Ok(())
        })
    }
}

pub struct Reader<R: io::Read> {
    read: io::BufReader<R>,
}

impl<R: io::Read> Reader<R> {
    fn read(&mut self) -> io::Result<Stats> {
        let mut prefix = [0u8, 0, 0];
        self.read.read_exact(&mut prefix)?;

        let id = prefix[0];
        let len = u16::from_le_bytes([prefix[1], prefix[2]]) as usize;

        BUFFER.with(|cell| {
            let mut buffer = cell.borrow_mut();
            let buffer = &mut *buffer;
            buffer.resize(len, 0);

            self.read.read_exact(buffer)?;

            let buffer = io::Cursor::new(buffer);

            match id {
                0 => {
                    let msg = Parameters::decode(buffer)?;
                    Ok(msg.into())
                }
                1 => {
                    let msg = Connection::decode(buffer)?;
                    Ok(msg.into())
                }
                2 => {
                    let msg = Setup::decode(buffer)?;
                    Ok(msg.into())
                }
                _ => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid stat tag",
                )),
            }
        })
    }
}

impl<R: io::Read> Iterator for Reader<R> {
    type Item = io::Result<Stats>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.read() {
            Ok(value) => Some(Ok(value)),
            Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => None,
            Err(err) => Some(Err(err)),
        }
    }
}

#[derive(Clone, Message)]
pub struct Setup {
    #[prost(string, repeated, tag = "1")]
    pub args: Vec<String>,
}

impl From<Setup> for Stats {
    fn from(s: Setup) -> Self {
        Self::Setup(s)
    }
}

impl Setup {
    pub fn write<W: io::Write>(&self, w: W) -> io::Result<()> {
        Stats::write(w, 2, self)
    }
}

#[derive(Clone, Copy, Message)]
pub struct Parameters {
    #[prost(uint64, tag = "1")]
    pub seed: u64,
    #[prost(double, tag = "2")]
    pub drop_rate: f64,
    #[prost(double, tag = "3")]
    pub corrupt_rate: f64,
    #[prost(message, tag = "4")]
    pub jitter: Option<Duration>,
    #[prost(message, tag = "5")]
    pub network_jitter: Option<Duration>,
    #[prost(message, tag = "6")]
    pub delay: Option<Duration>,
    #[prost(double, tag = "7")]
    pub retransmit_rate: f64,
    #[prost(uint32, tag = "8")]
    pub max_udp_payload: u32,
    #[prost(uint64, tag = "9")]
    pub transmit_rate: u64,
    #[prost(uint64, tag = "10")]
    pub max_inflight: u64,
    #[prost(uint32, tag = "11")]
    pub servers: u32,
    #[prost(uint32, tag = "12")]
    pub clients: u32,
    #[prost(message, tag = "13")]
    pub end_time: Option<Duration>,
    #[prost(message, tag = "14")]
    pub inflight_delay: Option<Duration>,
    #[prost(uint64, tag = "15")]
    pub inflight_delay_threshold: u64,
}

impl From<Parameters> for Stats {
    fn from(p: Parameters) -> Self {
        Self::Parameters(p)
    }
}

impl Parameters {
    pub fn write<W: io::Write>(&self, w: W) -> io::Result<()> {
        Stats::write(w, 0, self)
    }
}

#[derive(Clone, Copy, Message)]
pub struct Connection {
    #[prost(uint64, optional, tag = "1")]
    pub client_id: Option<u64>,
    #[prost(uint64, optional, tag = "2")]
    pub server_id: Option<u64>,
    #[prost(uint64, tag = "3")]
    pub seed: u64,
    #[prost(message, tag = "4")]
    pub start_time: Option<Duration>,
    #[prost(message, tag = "5")]
    pub end_time: Option<Duration>,
    #[prost(message, tag = "6")]
    pub handshake: Option<Handshake>,
    #[prost(uint64, optional, tag = "7")]
    pub transport_error: Option<u64>,
    #[prost(uint64, optional, tag = "8")]
    pub application_error: Option<u64>,
    #[prost(bool, tag = "9")]
    pub idle_timer_error: bool,
    #[prost(bool, tag = "10")]
    pub handshake_duration_exceeded_error: bool,
    #[prost(bool, tag = "11")]
    pub unspecified_error: bool,
    #[prost(message, tag = "12")]
    pub tx: Option<Counts>,
    #[prost(message, tag = "13")]
    pub rx: Option<Counts>,
    #[prost(message, tag = "14")]
    pub loss: Option<Counts>,
    #[prost(uint64, tag = "15")]
    pub congestion: u64,
    #[prost(uint64, tag = "16")]
    pub max_cwin: u64,
    #[prost(uint64, tag = "17")]
    pub max_bytes_in_flight: u64,
    #[prost(message, tag = "18")]
    pub max_rtt: Option<Duration>,
    #[prost(message, tag = "19")]
    pub min_rtt: Option<Duration>,
    #[prost(message, tag = "20")]
    pub smoothed_rtt: Option<Duration>,
}

impl From<Connection> for Stats {
    fn from(c: Connection) -> Self {
        Self::Connection(c)
    }
}

impl Connection {
    pub fn write<W: io::Write>(&self, w: W) -> io::Result<()> {
        Stats::write(w, 1, self)
    }

    pub fn id(&self) -> u64 {
        self.client_id.or(self.server_id).expect("missing id")
    }

    pub fn is_success(&self) -> bool {
        !self.is_error()
    }

    pub fn is_error(&self) -> bool {
        self.transport_error.is_some()
            || self.application_error.is_some()
            || self.idle_timer_error
            || self.handshake_duration_exceeded_error
            || self.unspecified_error
    }

    pub fn duration(&self) -> Option<core::time::Duration> {
        self.end_time
            .unwrap_or_default()
            .as_duration()
            .checked_sub(self.start_time.unwrap_or_default().as_duration())
    }
}

#[derive(Clone, Copy, Message, PartialEq, Eq)]
pub struct Counts {
    #[prost(uint64, tag = "1")]
    pub initial: u64,
    #[prost(uint64, tag = "2")]
    pub handshake: u64,
    #[prost(uint64, tag = "3")]
    pub retry: u64,
    #[prost(uint64, tag = "4")]
    pub one_rtt: u64,
    #[prost(uint64, tag = "5")]
    pub stream_progress: u64,
    #[prost(uint64, tag = "6")]
    pub stream_data_blocked: u64,
    #[prost(uint64, tag = "7")]
    pub data_blocked: u64,
    #[prost(message, tag = "8")]
    pub stream_progress_start: Option<Duration>,
    #[prost(message, tag = "9")]
    pub stream_progress_end: Option<Duration>,
}

impl Counts {
    #[inline]
    pub fn inc_packet(&mut self, header: &events::PacketHeader) {
        match header {
            events::PacketHeader::Initial { .. } => self.initial += 1,
            events::PacketHeader::Handshake { .. } => self.handshake += 1,
            events::PacketHeader::OneRtt { .. } => self.one_rtt += 1,
            _ => {}
        }
    }

    #[inline]
    pub fn inc_frame(&mut self, frame: &events::Frame) {
        use events::Frame::*;
        match frame {
            DataBlocked { .. } => {
                self.data_blocked += 1;
            }
            StreamDataBlocked { .. } => {
                self.stream_data_blocked += 1;
            }
            _ => {}
        }
    }

    #[inline]
    pub fn packets(&self) -> u64 {
        self.initial + self.handshake + self.retry + self.one_rtt
    }

    #[inline]
    pub fn stream_progress(&mut self, now: Timestamp, bytes: usize) {
        self.stream_progress += bytes as u64;
        self.stream_progress_end = Some(now.duration_since_start().into());
        if self.stream_progress_start.is_none() {
            self.stream_progress_start = Some(now.duration_since_start().into());
        }
    }

    #[inline]
    pub fn stream_throughput(&self) -> Option<f64> {
        let bytes = self.stream_progress as f64;
        let start = self.stream_progress_start?.as_duration();
        let end = self.stream_progress_end?.as_duration();
        let duration = end - start;
        Some(bytes / duration.as_secs_f64())
    }
}

#[derive(Clone, Copy, Message, PartialEq, Eq)]
pub struct Handshake {
    #[prost(message, tag = "1")]
    pub complete: Option<Duration>,
    #[prost(message, tag = "2")]
    pub confirmed: Option<Duration>,
}

#[derive(Clone, Copy, Message, PartialEq, Eq)]
pub struct Duration {
    #[prost(uint64, tag = "1")]
    pub secs: u64,
    #[prost(uint32, tag = "2")]
    pub nanos: u32,
}

impl Duration {
    pub fn as_duration(self) -> core::time::Duration {
        self.into()
    }
}

impl From<core::time::Duration> for Duration {
    fn from(value: core::time::Duration) -> Self {
        Self {
            secs: value.as_secs(),
            nanos: value.subsec_nanos(),
        }
    }
}

impl From<Duration> for core::time::Duration {
    fn from(value: Duration) -> Self {
        Self::new(value.secs, value.nanos)
    }
}

type Q = fn(&Parameters, &Connection, &[Connection]) -> Option<f64>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Type {
    Integer,
    Percent,
    Duration,
    Throughput,
    Bool,
}

impl Type {
    pub fn format(&self, [_min, max]: [f64; 2]) -> &'static str {
        match self {
            Self::Integer => "~s",
            Self::Percent => "~%",
            Self::Duration if max > 2000.0 => "%M:%S",
            Self::Duration => "%Qms",
            Self::Throughput => "~s",
            Self::Bool => "c",
        }
    }

    pub fn is_duration(&self) -> bool {
        matches!(self, Self::Duration)
    }

    pub fn parse(&self, value: &str) -> anyhow::Result<f64> {
        match self {
            Self::Integer => {
                let value: u64 = value.parse()?;
                Ok(value as _)
            }
            Self::Percent => {
                let (value, mul) = if let Some(value) = value.strip_suffix('%') {
                    (value, 100.0)
                } else {
                    (value, 1.0)
                };

                if let Ok(value) = value.parse::<u64>() {
                    let value = value as f64;
                    Ok(value * mul)
                } else {
                    let value = value.parse::<f64>()?;
                    Ok(value * mul)
                }
            }
            Self::Duration => {
                if let Ok(v) = value.parse::<jiff::SignedDuration>() {
                    Ok(v.as_secs_f64())
                } else {
                    Ok(value.parse()?)
                }
            }
            Self::Throughput => {
                todo!()
            }
            Self::Bool => match value {
                "true" | "TRUE" | "1" => Ok(1.0),
                "false" | "FALSE" | "0" => Ok(0.0),
                _ => Err(anyhow::anyhow!("invalid bool: {:?}", value)),
            },
        }
    }
}

use Type::{Bool as B, Duration as T, Integer as I, Percent as P, Throughput as Tpt};

static QUERIES: &[(&str, Type, Q)] = &[
    ("conn.duration", T, |_params, conn, _conns| {
        let duration = conn.duration()?;
        Some(duration.as_secs_f64())
    }),
    ("conn.handshake.confirmed", T, |_params, conn, _conns| {
        let duration = conn
            .handshake
            .unwrap_or_default()
            .confirmed
            .unwrap_or_default()
            .as_duration()
            .checked_sub(conn.start_time.unwrap_or_default().as_duration())?;
        Some(duration.as_secs_f64())
    }),
    ("conn.handshake.complete", T, |_params, conn, _conns| {
        let duration = conn
            .handshake
            .unwrap_or_default()
            .complete
            .unwrap_or_default()
            .as_duration()
            .checked_sub(conn.start_time.unwrap_or_default().as_duration())?;
        Some(duration.as_secs_f64())
    }),
    ("conn.id", B, |_params, conn, _conns| {
        conn.client_id.or(conn.server_id).map(|v| v as f64)
    }),
    ("conn.client", B, |_params, conn, _conns| {
        Some(if conn.client_id.is_some() { 1.0 } else { 0.0 })
    }),
    ("conn.server", B, |_params, conn, _conns| {
        Some(if conn.server_id.is_some() { 1.0 } else { 0.0 })
    }),
    ("conn.success", B, |_params, conn, _conns| {
        Some(if conn.is_success() { 1.0 } else { 0.0 })
    }),
    ("conn.error", B, |_params, conn, _conns| {
        Some(if conn.is_error() { 1.0 } else { 0.0 })
    }),
    ("conn.congestion", I, |_params, conn, _conns| {
        Some(conn.congestion as _)
    }),
    ("conn.max-cwin", I, |_params, conn, _conns| {
        Some(conn.max_cwin as _)
    }),
    ("conn.max-bytes-in-flight", I, |_params, conn, _conns| {
        Some(conn.max_bytes_in_flight as _)
    }),
    ("conn.min-rtt", T, |_params, conn, _conns| {
        Some(conn.min_rtt?.as_duration().as_secs_f64())
    }),
    ("conn.max-rtt", T, |_params, conn, _conns| {
        Some(conn.max_rtt?.as_duration().as_secs_f64())
    }),
    ("conn.smoothed-rtt", T, |_params, conn, _conns| {
        Some(conn.smoothed_rtt?.as_duration().as_secs_f64())
    }),
    ("conn.rtt-spread", T, |_params, conn, _conns| {
        let min = conn.min_rtt?.as_duration();
        let max = conn.max_rtt?.as_duration();
        let variance = max - min;
        Some(variance.as_secs_f64())
    }),
    ("conn.tx.packets", I, |_params, conn, _conns| {
        Some(conn.tx.unwrap_or_default().packets() as _)
    }),
    ("conn.tx.initial", I, |_params, conn, _conns| {
        Some(conn.tx.unwrap_or_default().initial as _)
    }),
    ("conn.tx.handshake", I, |_params, conn, _conns| {
        Some(conn.tx.unwrap_or_default().handshake as _)
    }),
    ("conn.tx.one-rtt", I, |_params, conn, _conns| {
        Some(conn.tx.unwrap_or_default().one_rtt as _)
    }),
    ("conn.tx.data-blocked", I, |_params, conn, _conns| {
        Some(conn.tx.unwrap_or_default().data_blocked as _)
    }),
    ("conn.tx.stream-data-blocked", I, |_params, conn, _conns| {
        Some(conn.tx.unwrap_or_default().stream_data_blocked as _)
    }),
    ("conn.tx.stream-progress", I, |_params, conn, _conns| {
        Some(conn.tx.unwrap_or_default().stream_progress as _)
    }),
    ("conn.tx.stream-throughput", Tpt, |_params, conn, _conns| {
        conn.tx?.stream_throughput()
    }),
    ("conn.rx.packets", I, |_params, conn, _conns| {
        Some(conn.rx.unwrap_or_default().packets() as _)
    }),
    ("conn.rx.initial", I, |_params, conn, _conns| {
        Some(conn.rx.unwrap_or_default().initial as _)
    }),
    ("conn.rx.handshake", I, |_params, conn, _conns| {
        Some(conn.rx.unwrap_or_default().handshake as _)
    }),
    ("conn.rx.one-rtt", I, |_params, conn, _conns| {
        Some(conn.rx.unwrap_or_default().one_rtt as _)
    }),
    ("conn.rx.data-blocked", I, |_params, conn, _conns| {
        Some(conn.rx.unwrap_or_default().data_blocked as _)
    }),
    ("conn.rx.stream-data-blocked", I, |_params, conn, _conns| {
        Some(conn.rx.unwrap_or_default().stream_data_blocked as _)
    }),
    ("conn.rx.stream-progress", I, |_params, conn, _conns| {
        Some(conn.rx.unwrap_or_default().stream_progress as _)
    }),
    ("conn.rx.stream-throughput", Tpt, |_params, conn, _conns| {
        conn.rx?.stream_throughput()
    }),
    ("conn.lost.packets", I, |_params, conn, _conns| {
        Some(conn.loss.unwrap_or_default().packets() as _)
    }),
    ("conn.lost.initial", I, |_params, conn, _conns| {
        Some(conn.loss.unwrap_or_default().initial as _)
    }),
    ("conn.lost.handshake", I, |_params, conn, _conns| {
        Some(conn.loss.unwrap_or_default().handshake as _)
    }),
    ("conn.lost.one-rtt", I, |_params, conn, _conns| {
        Some(conn.loss.unwrap_or_default().one_rtt as _)
    }),
    ("sim.success", I, |_params, conn, conns| {
        // only return the value for the first connection
        if conn.id() == conns[0].id() {
            Some(conns.iter().filter(|c| c.is_success()).count() as _)
        } else {
            None
        }
    }),
    ("sim.error", I, |_params, conn, conns| {
        // only return the value for the first connection
        if conn.id() == conns[0].id() {
            Some(conns.iter().filter(|c| c.is_error()).count() as _)
        } else {
            None
        }
    }),
    ("net.drop-rate", P, |params, _conn, _conns| {
        Some(params.drop_rate)
    }),
    ("net.corrupt-rate", P, |params, _conn, _conns| {
        Some(params.corrupt_rate)
    }),
    ("net.jitter", T, |params, _conn, _conns| {
        Some(
            params
                .jitter
                .unwrap_or_default()
                .as_duration()
                .as_secs_f64(),
        )
    }),
    ("net.network-jitter", T, |params, _conn, _conns| {
        Some(
            params
                .network_jitter
                .unwrap_or_default()
                .as_duration()
                .as_secs_f64(),
        )
    }),
    ("net.delay", T, |params, _conn, _conns| {
        Some(params.delay.unwrap_or_default().as_duration().as_secs_f64())
    }),
    ("net.transmit-rate", I, |params, _conn, _conns| {
        Some(params.transmit_rate as f64)
    }),
    ("net.retransmit-rate", P, |params, _conn, _conns| {
        Some(params.retransmit_rate)
    }),
    ("net.max-udp-payload", I, |params, _conn, _conns| {
        Some(params.max_udp_payload as f64)
    }),
    ("net.max-inflight", I, |params, _conn, _conns| {
        Some(params.max_inflight as f64)
    }),
    ("net.endpoints", I, |params, _conn, _conns| {
        Some((params.servers + params.clients) as f64)
    }),
    ("net.servers", I, |params, _conn, _conns| {
        Some(params.servers as f64)
    }),
    ("net.clients", I, |params, _conn, _conns| {
        Some(params.clients as f64)
    }),
    ("net.connections", I, |_params, _conn, conns| {
        Some(conns.len() as f64)
    }),
    (
        "net.inflight-delay-threshold",
        I,
        |params, _conn, _conns| Some(params.inflight_delay_threshold as f64),
    ),
    ("net.inflight-delay", T, |params, _conn, _conns| {
        Some(
            params
                .inflight_delay
                .unwrap_or_default()
                .as_duration()
                .as_secs_f64(),
        )
    }),
];

pub static QUERY_NAMES: Lazy<Vec<&'static str>> =
    Lazy::new(|| QUERIES.iter().map(|q| q.0).collect());

#[derive(Clone, Copy)]
pub struct Query {
    pub name: &'static str,
    pub ty: Type,
    pub query: Q,
}

impl fmt::Debug for Query {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Query")
            .field("name", &self.name)
            .field("type", &self.ty)
            .finish()
    }
}

impl fmt::Display for Query {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.name.fmt(f)
    }
}

impl Query {
    pub fn apply(
        &self,
        params: &Parameters,
        conn: &Connection,
        conns: &[Connection],
    ) -> Option<f64> {
        (self.query)(params, conn, conns)
    }
}

impl FromStr for Query {
    type Err = anyhow::Error;

    fn from_str(path: &str) -> Result<Self, Self::Err> {
        for (name, ty, query) in QUERIES.iter().copied() {
            if name
                .split('-')
                .eq(path.split('_').flat_map(|v| v.split('-')))
            {
                return Ok(Self { name, ty, query });
            }
        }

        Err(anyhow::anyhow!("invalid query: {}", path))
    }
}

struct StrVisitor<T>(core::marker::PhantomData<T>);

impl<T> serde::de::Visitor<'_> for StrVisitor<T>
where
    T: FromStr,
    <T as FromStr>::Err: core::fmt::Display,
{
    type Value = T;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(formatter, "a valid thing")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        v.parse().map_err(E::custom)
    }
}

impl<'de> serde::Deserialize<'de> for Query {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(StrVisitor::<Self>(Default::default()))
    }
}

type Op = fn(f64, f64) -> bool;

#[derive(Clone)]
pub struct Filter {
    pub expr: String,
    pub query: Query,
    pub value: f64,
    pub op: Op,
}

impl fmt::Debug for Filter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Filter")
            .field("query", &self.query)
            .field("value", &self.value)
            .finish()
    }
}

impl fmt::Display for Filter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.expr.fmt(f)
    }
}

impl Filter {
    pub fn apply(&self, params: &Parameters, conn: &Connection, conns: &[Connection]) -> bool {
        self.query
            .apply(params, conn, conns)
            .is_some_and(|actual| (self.op)(actual, self.value))
    }
}

impl FromStr for Filter {
    type Err = anyhow::Error;

    fn from_str(filter: &str) -> Result<Self, Self::Err> {
        let (query, op, value): (_, Op, _) = if let Some((q, v)) = filter.split_once("!=") {
            (q, |a, b| (a - b).abs() > f64::EPSILON, v)
        } else if let Some((q, v)) = filter.split_once("==") {
            (q, |a, b| (a - b).abs() < f64::EPSILON, v)
        } else if let Some((q, v)) = filter.split_once(">=") {
            (q, |a, b| a >= b, v)
        } else if let Some((q, v)) = filter.split_once('>') {
            (q, |a, b| a > b, v)
        } else if let Some((q, v)) = filter.split_once("<=") {
            (q, |a, b| a <= b, v)
        } else if let Some((q, v)) = filter.split_once('<') {
            (q, |a, b| a < b, v)
        } else if let Some((q, v)) = filter.split_once('=') {
            (q, |a, b| (a - b).abs() < f64::EPSILON, v)
        } else {
            (filter, |a, _b| a != 0.0, "0")
        };

        let query = Query::from_str(query)?;
        let value = query.ty.parse(value)?;

        Ok(Self {
            expr: filter.to_owned(),
            query,
            value,
            op,
        })
    }
}

impl<'de> serde::Deserialize<'de> for Filter {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(StrVisitor::<Self>(Default::default()))
    }
}
