// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[event("endpoint:initialized")]
#[subject(endpoint)]
struct EndpointInitialized<'a> {
    #[nominal_counter("acceptor.protocol")]
    acceptor_addr: SocketAddress<'a>,
    #[nominal_counter("handshake.protocol")]
    handshake_addr: SocketAddress<'a>,
    #[bool_counter("tcp")]
    tcp: bool,
    #[bool_counter("udp")]
    udp: bool,
}

#[event("dc:connection_timeout")]
#[subject(endpoint)]
/// Emitted when the DC handshake confirmation or MTU probing times out
struct DcConnectionTimeout<'a> {
    #[nominal_counter("peer_address.protocol")]
    peer_address: SocketAddress<'a>,
}

#[event("runtime:offload_metrics")]
#[subject(endpoint)]
struct OffloadRuntimeMetrics {
    #[measure("global_queue_depth", Count)]
    global_queue_depth: usize,
    #[measure("num_alive_tasks", Count)]
    num_alive_tasks: usize,
}

#[event("runtime:offload_worker_metrics")]
#[subject(endpoint)]
struct OffloadRuntimeWorkerMetrics {
    #[measure("park_count", Count)]
    park_count: u64,
    #[measure("busy_duration", Duration)]
    busy_duration: core::time::Duration,
}
