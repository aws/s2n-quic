// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, path::Path};

/// Root configuration for the RPC tester
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub endpoint: EndpointConfig,

    #[serde(default)]
    pub server: ServerConfig,

    #[serde(default)]
    pub client: ClientConfig,
}

/// Shared endpoint configuration (applies to both client and server)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointConfig {
    /// Number of workers for the send pipeline
    #[serde(default = "EndpointConfig::default_send_workers")]
    pub send_workers: usize,

    /// Number of workers for recv IO (socket read + decode)
    #[serde(default = "EndpointConfig::default_recv_io_workers")]
    pub recv_io_workers: usize,

    /// Number of workers for recv dispatch (decrypt + dedup + routing)
    #[serde(default = "EndpointConfig::default_recv_dispatch_workers")]
    pub recv_dispatch_workers: usize,

    /// Number of workers for waker drain (offloaded wake syscalls)
    #[serde(default = "EndpointConfig::default_waker_drain_workers")]
    pub waker_drain_workers: usize,

    /// Number of send sockets
    #[serde(default = "EndpointConfig::default_send_sockets")]
    pub send_sockets: usize,

    /// Overall bandwidth limit in Gbps
    #[serde(default = "EndpointConfig::default_bandwidth")]
    pub bandwidth: f64,

    /// Per-socket bandwidth limit in Gbps
    #[serde(default = "EndpointConfig::default_per_socket_bandwidth")]
    pub per_socket_bandwidth: f64,

    /// Number of shards for the frame submission channel (must be power of two)
    #[serde(default = "EndpointConfig::default_submission_shards")]
    pub submission_shards: usize,
}

impl EndpointConfig {
    fn default_send_workers() -> usize {
        4
    }

    fn default_recv_io_workers() -> usize {
        4
    }

    fn default_recv_dispatch_workers() -> usize {
        5
    }

    fn default_waker_drain_workers() -> usize {
        1
    }

    fn default_send_sockets() -> usize {
        64
    }

    fn default_bandwidth() -> f64 {
        25.0
    }

    fn default_per_socket_bandwidth() -> f64 {
        5.0
    }

    fn default_submission_shards() -> usize {
        128
    }

    /// Total number of busy-poll threads needed (frame_dispatch + all worker roles).
    pub fn total_workers(&self) -> usize {
        1 + self.send_workers
            + self.recv_io_workers
            + self.recv_dispatch_workers
            + self.waker_drain_workers
    }

    /// Constructs the worker layout, assigning contiguous thread indices to each role.
    pub fn layout(&self) -> s2n_quic_dc::stream3::endpoint::WorkerLayout {
        let mut ids = 1..; // 0 is frame_dispatch
        s2n_quic_dc::stream3::endpoint::WorkerLayout {
            frame_dispatch: 0,
            send: (&mut ids).take(self.send_workers).collect(),
            recv_io: (&mut ids).take(self.recv_io_workers).collect(),
            recv_dispatch: (&mut ids).take(self.recv_dispatch_workers).collect(),
            waker_drain: (&mut ids).take(self.waker_drain_workers).collect(),
        }
    }
}

impl Default for EndpointConfig {
    fn default() -> Self {
        Self {
            send_workers: Self::default_send_workers(),
            recv_io_workers: Self::default_recv_io_workers(),
            recv_dispatch_workers: Self::default_recv_dispatch_workers(),
            waker_drain_workers: Self::default_waker_drain_workers(),
            send_sockets: Self::default_send_sockets(),
            bandwidth: Self::default_bandwidth(),
            per_socket_bandwidth: Self::default_per_socket_bandwidth(),
            submission_shards: Self::default_submission_shards(),
        }
    }
}

impl Config {
    /// Load configuration from a TOML file
    pub fn load(path: &Path) -> std::io::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        toml::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}

/// Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    /// The server's address — clients use this to connect.
    /// Data routing is discovered automatically.
    #[serde(default = "ServerConfig::default_address")]
    pub address: SocketAddr,
}

impl ServerConfig {
    fn default_address() -> SocketAddr {
        "[::]:4433".parse().unwrap()
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            address: Self::default_address(),
        }
    }
}

/// Client configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientConfig {
    /// List of workload configurations
    #[serde(default = "ClientConfig::default_workloads")]
    pub workloads: Vec<WorkloadConfig>,
}

impl ClientConfig {
    fn default_workloads() -> Vec<WorkloadConfig> {
        vec![WorkloadConfig::default()]
    }
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            workloads: Self::default_workloads(),
        }
    }
}

/// Configuration for a single workload type
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkloadConfig {
    /// Human-readable name for this workload
    #[serde(default = "WorkloadConfig::default_name")]
    pub name: String,

    /// Number of concurrent workers running this workload
    #[serde(default = "WorkloadConfig::default_workers")]
    pub workers: usize,

    /// Size of the request body in bytes
    #[serde(default = "WorkloadConfig::default_request_size")]
    pub request_size: u64,

    /// Size of the response body in bytes
    #[serde(default = "WorkloadConfig::default_response_size")]
    pub response_size: u64,

    /// Delay between requests in milliseconds (0 means continuous)
    #[serde(default)]
    pub request_delay_ms: u64,
}

impl WorkloadConfig {
    fn default_name() -> String {
        "default".into()
    }

    fn default_workers() -> usize {
        1
    }

    fn default_request_size() -> u64 {
        1024
    }

    fn default_response_size() -> u64 {
        1024
    }
}

impl Default for WorkloadConfig {
    fn default() -> Self {
        Self {
            name: Self::default_name(),
            workers: Self::default_workers(),
            request_size: Self::default_request_size(),
            response_size: Self::default_response_size(),
            request_delay_ms: 0,
        }
    }
}
