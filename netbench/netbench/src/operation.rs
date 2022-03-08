// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::units::{duration_format, Byte, Duration, Rate};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Connection {
    /// Pause for the specified duration before processing the next op
    Sleep {
        #[serde(with = "duration_format", rename = "amount_ms")]
        amount: Duration,
    },
    /// Open a bidirectional stream with an identifier
    OpenBidirectionalStream { stream_id: u64 },
    /// Open a unidirectional stream with an identifier
    OpenSendStream { stream_id: u64 },
    /// Send a specific amount of data over the stream id
    Send { stream_id: u64, bytes: Byte },
    /// Finish sending data on the stream
    SendFinish { stream_id: u64 },
    /// Sets the send rate for a stream
    SendRate {
        stream_id: u64,
        #[serde(flatten)]
        rate: Rate,
    },
    /// Send a specific amount of data over the stream id
    Receive { stream_id: u64, bytes: Byte },
    /// Receives all of the data on the stream until it is finished
    ReceiveAll { stream_id: u64 },
    /// Finish receiving data on the stream
    ReceiveFinish { stream_id: u64 },
    /// Sets the receive rate for a stream
    ReceiveRate {
        stream_id: u64,
        #[serde(flatten)]
        rate: Rate,
    },
    /// Parks the current thread and waits for the checkpoint to be unparked
    Park { checkpoint: u64 },
    /// Notifies the parked checkpoint that it can continue
    Unpark { checkpoint: u64 },
    /// Emit a trace event
    Trace { trace_id: u64 },
    /// Perform operations concurrently
    Scope { threads: Vec<Vec<Connection>> },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Client {
    /// Pause for the specified duration before processing the next op
    Sleep {
        #[serde(with = "duration_format", rename = "timeout_ms")]
        timeout: Duration,
    },
    /// Open a connection with an identifier
    Connect {
        server_id: u64,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        router_id: Option<u64>,
        server_connection_id: u64,
        client_connection_id: u64,
    },
    /// Parks the current thread and waits for the checkpoint to be unparked
    Park { checkpoint: u64 },
    /// Notifies the parked checkpoint that it can continue
    Unpark { checkpoint: u64 },
    /// Emit a trace event
    Trace { trace_id: u64 },
    /// Perform operations concurrently
    Scope { threads: Vec<Vec<Client>> },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Router {
    /// Pause for the specified duration before processing the next op
    Sleep {
        #[serde(with = "duration_format", rename = "amount_ms")]
        amount: Duration,
    },

    /// Set the number of packets that can be buffered server->client
    ServerBufferCount { packet_count: u32 },
    /// Set the chance of a server->client packet being dropped
    ServerDropRate { packet_count: u32 },
    /// Set the chance of a server->client packet being reordered
    ServerReorderRate { packet_count: u32 },
    /// Set the chance of a server->client packet being corrupted
    ServerCorruptRate { packet_count: u32 },
    /// Set the amount of delay for server->client packets
    ServerDelay {
        #[serde(with = "duration_format", rename = "amount_ms")]
        amount: Duration,
    },
    /// Set the amount of jitter for server->client packets
    ServerJitter {
        #[serde(with = "duration_format", rename = "amount_ms")]
        amount: Duration,
    },
    /// Set the server->client MTU
    ServerMtu { mtu: u16 },

    /// Set the number of packets that can be buffered server->client
    ClientBufferCount { packet_count: u32 },
    /// Set the chance of a client->server packet being dropped
    ClientDropRate { packet_count: u32 },
    /// Set the chance of a client->server packet being reordered
    ClientReorderRate { packet_count: u32 },
    /// Set the chance of a client->server packet being corrupted
    ClientCorruptRate { packet_count: u32 },
    /// Set the amount of delay for client->server packets
    ClientDelay {
        #[serde(with = "duration_format", rename = "amount_ms")]
        amount: Duration,
    },
    /// Set the amount of jitter for client->server packets
    ClientJitter {
        #[serde(with = "duration_format", rename = "amount_ms")]
        amount: Duration,
    },
    /// Set the client->server MTU
    ClientMtu { mtu: u16 },

    /// Set the chance of a port being rebound
    ClientRebindPortRate { packet_count: u32 },
    /// Set the chance of an IP being rebound
    ClientRebindAddressRate { packet_count: u32 },

    /// Rebinds all of the ports and/or addresses currently being used
    RebindAll { ports: bool, addresses: bool },

    /// Emit a trace event
    Trace { trace_id: u64 },
}
