// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// DO NOT MODIFY THIS FILE
// This file was generated with the `s2n-quic-events` crate and any required
// changes should be made there.

use crate::event::{
    self, api,
    metrics::aggregate::{
        info::{self, Str},
        AsVariant, BoolRecorder, Info, Metric, NominalRecorder, Recorder, Registry, Units,
    },
};
use core::sync::atomic::{AtomicU64, Ordering};
static INFO: &[Info; 230usize] = &[
    info::Builder {
        id: 0usize,
        name: Str::new("acceptor_tcp_started\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 1usize,
        name: Str::new("acceptor_tcp_loop_iteration_completed\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 2usize,
        name: Str::new("acceptor_tcp_loop_iteration_completed.pending_streams\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 3usize,
        name: Str::new("acceptor_tcp_loop_iteration_completed.slots_idle\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 4usize,
        name: Str::new("acceptor_tcp_loop_iteration_completed.slot_utilization\0"),
        units: Units::Percent,
    }
    .build(),
    info::Builder {
        id: 5usize,
        name: Str::new("acceptor_tcp_loop_iteration_completed.processing_duration\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 6usize,
        name: Str::new("acceptor_tcp_loop_iteration_completed.max_sojourn_time\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 7usize,
        name: Str::new("acceptor_tcp_fresh_enqueued\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 8usize,
        name: Str::new("acceptor_tcp_fresh_batch_completed\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 9usize,
        name: Str::new("acceptor_tcp_fresh_batch_completed.enqueued\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 10usize,
        name: Str::new("acceptor_tcp_fresh_batch_completed.dropped\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 11usize,
        name: Str::new("acceptor_tcp_fresh_batch_completed.errored\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 12usize,
        name: Str::new("acceptor_tcp_stream_dropped\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 13usize,
        name: Str::new("acceptor_tcp_stream_dropped.reason\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 14usize,
        name: Str::new("acceptor_tcp_stream_replaced\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 15usize,
        name: Str::new("acceptor_tcp_stream_replaced.sojourn_time\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 16usize,
        name: Str::new("acceptor_tcp_stream_replaced.buffer_len\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 17usize,
        name: Str::new("acceptor_tcp_packet_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 18usize,
        name: Str::new("acceptor_tcp_packet_received.payload_len\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 19usize,
        name: Str::new("acceptor_tcp_packet_received.is_fin\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 20usize,
        name: Str::new("acceptor_tcp_packet_received.is_fin_known\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 21usize,
        name: Str::new("acceptor_tcp_packet_received.sojourn_time\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 22usize,
        name: Str::new("acceptor_tcp_packet_dropped\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 23usize,
        name: Str::new("acceptor_tcp_packet_dropped.reason\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 24usize,
        name: Str::new("acceptor_tcp_packet_dropped.sojourn_time\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 25usize,
        name: Str::new("acceptor_tcp_stream_enqueued\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 26usize,
        name: Str::new("acceptor_tcp_stream_enqueued.sojourn_time\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 27usize,
        name: Str::new("acceptor_tcp_stream_enqueued.blocked_count\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 28usize,
        name: Str::new("acceptor_tcp_io_error\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 29usize,
        name: Str::new("acceptor_udp_started\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 30usize,
        name: Str::new("acceptor_udp_datagram_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 31usize,
        name: Str::new("acceptor_udp_datagram_received.len\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 32usize,
        name: Str::new("acceptor_udp_packet_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 33usize,
        name: Str::new("acceptor_udp_packet_received.payload_len\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 34usize,
        name: Str::new("acceptor_udp_packet_received.is_zero_offset\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 35usize,
        name: Str::new("acceptor_udp_packet_received.is_retransmisson\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 36usize,
        name: Str::new("acceptor_udp_packet_received.is_fin\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 37usize,
        name: Str::new("acceptor_udp_packet_received.is_fin_known\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 38usize,
        name: Str::new("acceptor_udp_packet_dropped\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 39usize,
        name: Str::new("acceptor_udp_packet_dropped.reason\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 40usize,
        name: Str::new("acceptor_udp_stream_enqueued\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 41usize,
        name: Str::new("acceptor_udp_io_error\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 42usize,
        name: Str::new("acceptor_stream_pruned\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 43usize,
        name: Str::new("acceptor_stream_pruned.sojourn_time\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 44usize,
        name: Str::new("acceptor_stream_pruned.reason\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 45usize,
        name: Str::new("acceptor_stream_dequeued\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 46usize,
        name: Str::new("acceptor_stream_dequeued.sojourn_time\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 47usize,
        name: Str::new("stream_write_flushed\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 48usize,
        name: Str::new("stream_write_flushed.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 49usize,
        name: Str::new("stream_write_flushed.conn\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 50usize,
        name: Str::new("stream_write_flushed.provided\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 51usize,
        name: Str::new("stream_write_flushed.committed.total\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 52usize,
        name: Str::new("stream_write_flushed.committed\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 53usize,
        name: Str::new("stream_write_flushed.committed.conn\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 54usize,
        name: Str::new("stream_write_flushed.processing_duration\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 55usize,
        name: Str::new("stream_write_flushed.processing_duration.conn\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 56usize,
        name: Str::new("stream_write_fin_flushed\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 57usize,
        name: Str::new("stream_write_fin_flushed.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 58usize,
        name: Str::new("stream_write_fin_flushed.conn\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 59usize,
        name: Str::new("stream_write_fin_flushed.provided\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 60usize,
        name: Str::new("stream_write_fin_flushed.committed.total\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 61usize,
        name: Str::new("stream_write_fin_flushed.committed\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 62usize,
        name: Str::new("stream_write_fin_flushed.committed.conn\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 63usize,
        name: Str::new("stream_write_fin_flushed.processing_duration\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 64usize,
        name: Str::new("stream_write_fin_flushed.processing_duration.conn\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 65usize,
        name: Str::new("stream_write_blocked\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 66usize,
        name: Str::new("stream_write_blocked.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 67usize,
        name: Str::new("stream_write_blocked.conn\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 68usize,
        name: Str::new("stream_write_blocked.provided\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 69usize,
        name: Str::new("stream_write_blocked.processing_duration\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 70usize,
        name: Str::new("stream_write_blocked.processing_duration.conn\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 71usize,
        name: Str::new("stream_write_errored\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 72usize,
        name: Str::new("stream_write_errored.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 73usize,
        name: Str::new("stream_write_errored.provided\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 74usize,
        name: Str::new("stream_write_errored.processing_duration\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 75usize,
        name: Str::new("stream_write_errored.processing_duration.conn\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 76usize,
        name: Str::new("stream_write_key_updated\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 77usize,
        name: Str::new("stream_write_shutdown\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 78usize,
        name: Str::new("stream_write_shutdown.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 79usize,
        name: Str::new("stream_write_shutdown.buffer_len\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 80usize,
        name: Str::new("stream_write_shutdown.background\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 81usize,
        name: Str::new("stream_write_socket_flushed\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 82usize,
        name: Str::new("stream_write_socket_flushed.conn\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 83usize,
        name: Str::new("stream_write_socket_flushed.provided\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 84usize,
        name: Str::new("stream_write_socket_flushed.committed.total\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 85usize,
        name: Str::new("stream_write_socket_flushed.committed\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 86usize,
        name: Str::new("stream_write_socket_flushed.committed.conn\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 87usize,
        name: Str::new("stream_write_socket_blocked\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 88usize,
        name: Str::new("stream_write_socket_blocked.conn\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 89usize,
        name: Str::new("stream_write_socket_blocked.provided\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 90usize,
        name: Str::new("stream_write_socket_errored\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 91usize,
        name: Str::new("stream_write_socket_errored.provided\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 92usize,
        name: Str::new("stream_read_flushed\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 93usize,
        name: Str::new("stream_read_flushed.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 94usize,
        name: Str::new("stream_read_flushed.conn\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 95usize,
        name: Str::new("stream_read_flushed.capacity\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 96usize,
        name: Str::new("stream_read_flushed.committed.total\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 97usize,
        name: Str::new("stream_read_flushed.committed\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 98usize,
        name: Str::new("stream_read_flushed.committed.conn\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 99usize,
        name: Str::new("stream_read_flushed.processing_duration\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 100usize,
        name: Str::new("stream_read_flushed.processing_duration.conn\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 101usize,
        name: Str::new("stream_read_fin_flushed\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 102usize,
        name: Str::new("stream_read_fin_flushed.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 103usize,
        name: Str::new("stream_read_fin_flushed.conn\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 104usize,
        name: Str::new("stream_read_fin_flushed.capacity\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 105usize,
        name: Str::new("stream_read_fin_flushed.processing_duration\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 106usize,
        name: Str::new("stream_read_fin_flushed.processing_duration.conn\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 107usize,
        name: Str::new("stream_read_blocked\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 108usize,
        name: Str::new("stream_read_blocked.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 109usize,
        name: Str::new("stream_read_blocked.capacity\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 110usize,
        name: Str::new("stream_read_blocked.processing_duration\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 111usize,
        name: Str::new("stream_read_blocked.processing_duration.conn\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 112usize,
        name: Str::new("stream_read_errored\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 113usize,
        name: Str::new("stream_read_errored.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 114usize,
        name: Str::new("stream_read_errored.capacity\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 115usize,
        name: Str::new("stream_read_errored.processing_duration\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 116usize,
        name: Str::new("stream_read_errored.processing_duration.conn\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 117usize,
        name: Str::new("stream_read_key_updated\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 118usize,
        name: Str::new("stream_read_shutdown\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 119usize,
        name: Str::new("stream_read_shutdown.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 120usize,
        name: Str::new("stream_read_shutdown.background\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 121usize,
        name: Str::new("stream_read_socket_flushed\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 122usize,
        name: Str::new("stream_read_socket_flushed.conn\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 123usize,
        name: Str::new("stream_read_socket_flushed.capacity\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 124usize,
        name: Str::new("stream_read_socket_flushed.committed.total\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 125usize,
        name: Str::new("stream_read_socket_flushed.committed\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 126usize,
        name: Str::new("stream_read_socket_flushed.committed.conn\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 127usize,
        name: Str::new("stream_read_socket_blocked\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 128usize,
        name: Str::new("stream_read_socket_blocked.conn\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 129usize,
        name: Str::new("stream_read_socket_blocked.capacity\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 130usize,
        name: Str::new("stream_read_socket_errored\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 131usize,
        name: Str::new("stream_read_socket_errored.capacity\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 132usize,
        name: Str::new("stream_tcp_connect\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 133usize,
        name: Str::new("stream_tcp_connect.error\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 134usize,
        name: Str::new("stream_tcp_connect.tcp_latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 135usize,
        name: Str::new("stream_connect\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 136usize,
        name: Str::new("stream_connect.error\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 137usize,
        name: Str::new("stream_connect.tcp\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 138usize,
        name: Str::new("stream_connect.handshake\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 139usize,
        name: Str::new("stream_connect_error\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 140usize,
        name: Str::new("stream_connect_error.reason\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 141usize,
        name: Str::new("connection_closed\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 142usize,
        name: Str::new("endpoint_initialized\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 143usize,
        name: Str::new("endpoint_initialized.acceptor.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 144usize,
        name: Str::new("endpoint_initialized.handshake.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 145usize,
        name: Str::new("endpoint_initialized.tcp\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 146usize,
        name: Str::new("endpoint_initialized.udp\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 147usize,
        name: Str::new("path_secret_map_initialized\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 148usize,
        name: Str::new("path_secret_map_initialized.capacity\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 149usize,
        name: Str::new("path_secret_map_uninitialized\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 150usize,
        name: Str::new("path_secret_map_uninitialized.capacity\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 151usize,
        name: Str::new("path_secret_map_uninitialized.entries\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 152usize,
        name: Str::new("path_secret_map_uninitialized.lifetime\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 153usize,
        name: Str::new("path_secret_map_background_handshake_requested\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 154usize,
        name: Str::new("path_secret_map_background_handshake_requested.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 155usize,
        name: Str::new("path_secret_map_entry_inserted\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 156usize,
        name: Str::new("path_secret_map_entry_inserted.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 157usize,
        name: Str::new("path_secret_map_entry_ready\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 158usize,
        name: Str::new("path_secret_map_entry_ready.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 159usize,
        name: Str::new("path_secret_map_entry_replaced\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 160usize,
        name: Str::new("path_secret_map_entry_replaced.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 161usize,
        name: Str::new("path_secret_map_id_entry_evicted\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 162usize,
        name: Str::new("path_secret_map_id_entry_evicted.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 163usize,
        name: Str::new("path_secret_map_id_entry_evicted.age\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 164usize,
        name: Str::new("path_secret_map_address_entry_evicted\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 165usize,
        name: Str::new("path_secret_map_address_entry_evicted.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 166usize,
        name: Str::new("path_secret_map_address_entry_evicted.age\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 167usize,
        name: Str::new("unknown_path_secret_packet_sent\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 168usize,
        name: Str::new("unknown_path_secret_packet_sent.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 169usize,
        name: Str::new("unknown_path_secret_packet_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 170usize,
        name: Str::new("unknown_path_secret_packet_received.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 171usize,
        name: Str::new("unknown_path_secret_packet_accepted\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 172usize,
        name: Str::new("unknown_path_secret_packet_accepted.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 173usize,
        name: Str::new("unknown_path_secret_packet_rejected\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 174usize,
        name: Str::new("unknown_path_secret_packet_rejected.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 175usize,
        name: Str::new("unknown_path_secret_packet_dropped\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 176usize,
        name: Str::new("unknown_path_secret_packet_dropped.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 177usize,
        name: Str::new("key_accepted\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 178usize,
        name: Str::new("key_accepted.gap\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 179usize,
        name: Str::new("key_accepted.forward_shift\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 180usize,
        name: Str::new("replay_definitely_detected\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 181usize,
        name: Str::new("replay_potentially_detected\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 182usize,
        name: Str::new("replay_potentially_detected.gap\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 183usize,
        name: Str::new("replay_detected_packet_sent\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 184usize,
        name: Str::new("replay_detected_packet_sent.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 185usize,
        name: Str::new("replay_detected_packet_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 186usize,
        name: Str::new("replay_detected_packet_received.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 187usize,
        name: Str::new("replay_detected_packet_accepted\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 188usize,
        name: Str::new("replay_detected_packet_accepted.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 189usize,
        name: Str::new("replay_detected_packet_rejected\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 190usize,
        name: Str::new("replay_detected_packet_rejected.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 191usize,
        name: Str::new("replay_detected_packet_dropped\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 192usize,
        name: Str::new("replay_detected_packet_dropped.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 193usize,
        name: Str::new("stale_key_packet_sent\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 194usize,
        name: Str::new("stale_key_packet_sent.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 195usize,
        name: Str::new("stale_key_packet_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 196usize,
        name: Str::new("stale_key_packet_received.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 197usize,
        name: Str::new("stale_key_packet_accepted\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 198usize,
        name: Str::new("stale_key_packet_accepted.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 199usize,
        name: Str::new("stale_key_packet_rejected\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 200usize,
        name: Str::new("stale_key_packet_rejected.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 201usize,
        name: Str::new("stale_key_packet_dropped\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 202usize,
        name: Str::new("stale_key_packet_dropped.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 203usize,
        name: Str::new("path_secret_map_address_cache_accessed\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 204usize,
        name: Str::new("path_secret_map_address_cache_accessed.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 205usize,
        name: Str::new("path_secret_map_address_cache_accessed.hit\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 206usize,
        name: Str::new("path_secret_map_address_cache_accessed_hit\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 207usize,
        name: Str::new("path_secret_map_address_cache_accessed_hit.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 208usize,
        name: Str::new("path_secret_map_address_cache_accessed_hit.age\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 209usize,
        name: Str::new("path_secret_map_id_cache_accessed\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 210usize,
        name: Str::new("path_secret_map_id_cache_accessed.hit\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 211usize,
        name: Str::new("path_secret_map_id_cache_accessed_hit\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 212usize,
        name: Str::new("path_secret_map_id_cache_accessed_hit.age\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 213usize,
        name: Str::new("path_secret_map_cleaner_cycled\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 214usize,
        name: Str::new("path_secret_map_cleaner_cycled.entries.id\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 215usize,
        name: Str::new("path_secret_map_cleaner_cycled.entries.id.retired\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 216usize,
        name: Str::new("path_secret_map_cleaner_cycled.entries.id.active\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 217usize,
        name: Str::new("path_secret_map_cleaner_cycled.entries.id.active.utilization\0"),
        units: Units::Percent,
    }
    .build(),
    info::Builder {
        id: 218usize,
        name: Str::new("path_secret_map_cleaner_cycled.entries.id.utilization\0"),
        units: Units::Percent,
    }
    .build(),
    info::Builder {
        id: 219usize,
        name: Str::new("path_secret_map_cleaner_cycled.entries.id.utilization.initial\0"),
        units: Units::Percent,
    }
    .build(),
    info::Builder {
        id: 220usize,
        name: Str::new("path_secret_map_cleaner_cycled.entries.address\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 221usize,
        name: Str::new("path_secret_map_cleaner_cycled.entries.address.active\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 222usize,
        name: Str::new("path_secret_map_cleaner_cycled.entries.address.active.utilization\0"),
        units: Units::Percent,
    }
    .build(),
    info::Builder {
        id: 223usize,
        name: Str::new("path_secret_map_cleaner_cycled.entries.address.retired\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 224usize,
        name: Str::new("path_secret_map_cleaner_cycled.entries.address.utilization\0"),
        units: Units::Percent,
    }
    .build(),
    info::Builder {
        id: 225usize,
        name: Str::new("path_secret_map_cleaner_cycled.entries.address.utilization.initial\0"),
        units: Units::Percent,
    }
    .build(),
    info::Builder {
        id: 226usize,
        name: Str::new("path_secret_map_cleaner_cycled.handshake_requests\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 227usize,
        name: Str::new("path_secret_map_cleaner_cycled.handshake_requests.retired\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 228usize,
        name: Str::new("path_secret_map_cleaner_cycled.handshake_lock_duration\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 229usize,
        name: Str::new("path_secret_map_cleaner_cycled.total_duration\0"),
        units: Units::Duration,
    }
    .build(),
];
#[derive(Debug)]
#[allow(dead_code)]
pub struct ConnectionContext {
    start_time: crate::event::Timestamp,
    ctr_12: AtomicU64,
    ctr_15: AtomicU64,
    ctr_17: AtomicU64,
    ctr_18: AtomicU64,
    ctr_21: AtomicU64,
    ctr_23: AtomicU64,
    ctr_24: AtomicU64,
    ctr_27: AtomicU64,
    ctr_30: AtomicU64,
    ctr_32: AtomicU64,
    ctr_35: AtomicU64,
    ctr_36: AtomicU64,
    ctr_39: AtomicU64,
    ctr_42: AtomicU64,
    ctr_44: AtomicU64,
    ctr_45: AtomicU64,
    ctr_48: AtomicU64,
    ctr_51: AtomicU64,
    ctr_54: AtomicU64,
    ctr_55: AtomicU64,
    ctr_58: AtomicU64,
    ctr_59: AtomicU64,
}
pub struct Subscriber<R: Registry> {
    #[allow(dead_code)]
    counters: Box<[R::Counter; 77usize]>,
    #[allow(dead_code)]
    bool_counters: Box<[R::BoolCounter; 14usize]>,
    #[allow(dead_code)]
    nominal_counters: Box<[R::NominalCounter]>,
    #[allow(dead_code)]
    nominal_counter_offsets: Box<[usize; 32usize]>,
    #[allow(dead_code)]
    measures: Box<[R::Measure; 89usize]>,
    #[allow(dead_code)]
    gauges: Box<[R::Gauge; 0usize]>,
    #[allow(dead_code)]
    timers: Box<[R::Timer; 18usize]>,
    #[allow(dead_code)]
    nominal_timers: Box<[R::NominalTimer]>,
    #[allow(dead_code)]
    nominal_timer_offsets: Box<[usize; 0usize]>,
    #[allow(dead_code)]
    registry: R,
}
impl<R: Registry + Default> Default for Subscriber<R> {
    fn default() -> Self {
        Self::new(R::default())
    }
}
impl<R: Registry> Subscriber<R> {
    #[doc = r" Creates a new subscriber with the given registry"]
    #[doc = r""]
    #[doc = r" # Note"]
    #[doc = r""]
    #[doc = r" All of the recorders are registered on initialization and cached for the lifetime"]
    #[doc = r" of the subscriber."]
    #[allow(unused_mut)]
    #[inline]
    pub fn new(registry: R) -> Self {
        let mut counters = Vec::with_capacity(77usize);
        let mut bool_counters = Vec::with_capacity(14usize);
        let mut nominal_counters = Vec::with_capacity(32usize);
        let mut nominal_counter_offsets = Vec::with_capacity(32usize);
        let mut measures = Vec::with_capacity(89usize);
        let mut gauges = Vec::with_capacity(0usize);
        let mut timers = Vec::with_capacity(18usize);
        let mut nominal_timers = Vec::with_capacity(0usize);
        let mut nominal_timer_offsets = Vec::with_capacity(0usize);
        counters.push(registry.register_counter(&INFO[0usize]));
        counters.push(registry.register_counter(&INFO[1usize]));
        counters.push(registry.register_counter(&INFO[7usize]));
        counters.push(registry.register_counter(&INFO[8usize]));
        counters.push(registry.register_counter(&INFO[12usize]));
        counters.push(registry.register_counter(&INFO[14usize]));
        counters.push(registry.register_counter(&INFO[17usize]));
        counters.push(registry.register_counter(&INFO[22usize]));
        counters.push(registry.register_counter(&INFO[25usize]));
        counters.push(registry.register_counter(&INFO[28usize]));
        counters.push(registry.register_counter(&INFO[29usize]));
        counters.push(registry.register_counter(&INFO[30usize]));
        counters.push(registry.register_counter(&INFO[32usize]));
        counters.push(registry.register_counter(&INFO[38usize]));
        counters.push(registry.register_counter(&INFO[40usize]));
        counters.push(registry.register_counter(&INFO[41usize]));
        counters.push(registry.register_counter(&INFO[42usize]));
        counters.push(registry.register_counter(&INFO[45usize]));
        counters.push(registry.register_counter(&INFO[47usize]));
        counters.push(registry.register_counter(&INFO[51usize]));
        counters.push(registry.register_counter(&INFO[56usize]));
        counters.push(registry.register_counter(&INFO[60usize]));
        counters.push(registry.register_counter(&INFO[65usize]));
        counters.push(registry.register_counter(&INFO[71usize]));
        counters.push(registry.register_counter(&INFO[76usize]));
        counters.push(registry.register_counter(&INFO[77usize]));
        counters.push(registry.register_counter(&INFO[81usize]));
        counters.push(registry.register_counter(&INFO[84usize]));
        counters.push(registry.register_counter(&INFO[87usize]));
        counters.push(registry.register_counter(&INFO[90usize]));
        counters.push(registry.register_counter(&INFO[92usize]));
        counters.push(registry.register_counter(&INFO[96usize]));
        counters.push(registry.register_counter(&INFO[101usize]));
        counters.push(registry.register_counter(&INFO[107usize]));
        counters.push(registry.register_counter(&INFO[112usize]));
        counters.push(registry.register_counter(&INFO[117usize]));
        counters.push(registry.register_counter(&INFO[118usize]));
        counters.push(registry.register_counter(&INFO[121usize]));
        counters.push(registry.register_counter(&INFO[124usize]));
        counters.push(registry.register_counter(&INFO[127usize]));
        counters.push(registry.register_counter(&INFO[130usize]));
        counters.push(registry.register_counter(&INFO[132usize]));
        counters.push(registry.register_counter(&INFO[135usize]));
        counters.push(registry.register_counter(&INFO[139usize]));
        counters.push(registry.register_counter(&INFO[141usize]));
        counters.push(registry.register_counter(&INFO[142usize]));
        counters.push(registry.register_counter(&INFO[147usize]));
        counters.push(registry.register_counter(&INFO[149usize]));
        counters.push(registry.register_counter(&INFO[153usize]));
        counters.push(registry.register_counter(&INFO[155usize]));
        counters.push(registry.register_counter(&INFO[157usize]));
        counters.push(registry.register_counter(&INFO[159usize]));
        counters.push(registry.register_counter(&INFO[161usize]));
        counters.push(registry.register_counter(&INFO[164usize]));
        counters.push(registry.register_counter(&INFO[167usize]));
        counters.push(registry.register_counter(&INFO[169usize]));
        counters.push(registry.register_counter(&INFO[171usize]));
        counters.push(registry.register_counter(&INFO[173usize]));
        counters.push(registry.register_counter(&INFO[175usize]));
        counters.push(registry.register_counter(&INFO[177usize]));
        counters.push(registry.register_counter(&INFO[180usize]));
        counters.push(registry.register_counter(&INFO[181usize]));
        counters.push(registry.register_counter(&INFO[183usize]));
        counters.push(registry.register_counter(&INFO[185usize]));
        counters.push(registry.register_counter(&INFO[187usize]));
        counters.push(registry.register_counter(&INFO[189usize]));
        counters.push(registry.register_counter(&INFO[191usize]));
        counters.push(registry.register_counter(&INFO[193usize]));
        counters.push(registry.register_counter(&INFO[195usize]));
        counters.push(registry.register_counter(&INFO[197usize]));
        counters.push(registry.register_counter(&INFO[199usize]));
        counters.push(registry.register_counter(&INFO[201usize]));
        counters.push(registry.register_counter(&INFO[203usize]));
        counters.push(registry.register_counter(&INFO[206usize]));
        counters.push(registry.register_counter(&INFO[209usize]));
        counters.push(registry.register_counter(&INFO[211usize]));
        counters.push(registry.register_counter(&INFO[213usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[19usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[20usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[34usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[35usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[36usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[37usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[80usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[120usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[133usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[136usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[145usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[146usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[205usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[210usize]));
        {
            #[allow(unused_imports)]
            use api::*;
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <AcceptorTcpStreamDropReason as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[13usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <AcceptorPacketDropReason as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[23usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <AcceptorPacketDropReason as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[39usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <AcceptorStreamPruneReason as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[44usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <MaybeBoolCounter as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[137usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <MaybeBoolCounter as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[138usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <StreamTcpConnectErrorReason as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[140usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[143usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[144usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[154usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[156usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[158usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[160usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[162usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[165usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[168usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[170usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[172usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[174usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[176usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[184usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[186usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[188usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[190usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[192usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[194usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[196usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[198usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[200usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[202usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[204usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <SocketAddress as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[207usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
        }
        measures.push(registry.register_measure(&INFO[2usize]));
        measures.push(registry.register_measure(&INFO[3usize]));
        measures.push(registry.register_measure(&INFO[4usize]));
        measures.push(registry.register_measure(&INFO[6usize]));
        measures.push(registry.register_measure(&INFO[9usize]));
        measures.push(registry.register_measure(&INFO[10usize]));
        measures.push(registry.register_measure(&INFO[11usize]));
        measures.push(registry.register_measure(&INFO[16usize]));
        measures.push(registry.register_measure(&INFO[18usize]));
        measures.push(registry.register_measure(&INFO[27usize]));
        measures.push(registry.register_measure(&INFO[31usize]));
        measures.push(registry.register_measure(&INFO[33usize]));
        measures.push(registry.register_measure(&INFO[49usize]));
        measures.push(registry.register_measure(&INFO[50usize]));
        measures.push(registry.register_measure(&INFO[52usize]));
        measures.push(registry.register_measure(&INFO[53usize]));
        measures.push(registry.register_measure(&INFO[54usize]));
        measures.push(registry.register_measure(&INFO[55usize]));
        measures.push(registry.register_measure(&INFO[58usize]));
        measures.push(registry.register_measure(&INFO[59usize]));
        measures.push(registry.register_measure(&INFO[61usize]));
        measures.push(registry.register_measure(&INFO[62usize]));
        measures.push(registry.register_measure(&INFO[63usize]));
        measures.push(registry.register_measure(&INFO[64usize]));
        measures.push(registry.register_measure(&INFO[67usize]));
        measures.push(registry.register_measure(&INFO[68usize]));
        measures.push(registry.register_measure(&INFO[69usize]));
        measures.push(registry.register_measure(&INFO[70usize]));
        measures.push(registry.register_measure(&INFO[73usize]));
        measures.push(registry.register_measure(&INFO[74usize]));
        measures.push(registry.register_measure(&INFO[75usize]));
        measures.push(registry.register_measure(&INFO[79usize]));
        measures.push(registry.register_measure(&INFO[82usize]));
        measures.push(registry.register_measure(&INFO[83usize]));
        measures.push(registry.register_measure(&INFO[85usize]));
        measures.push(registry.register_measure(&INFO[86usize]));
        measures.push(registry.register_measure(&INFO[88usize]));
        measures.push(registry.register_measure(&INFO[89usize]));
        measures.push(registry.register_measure(&INFO[91usize]));
        measures.push(registry.register_measure(&INFO[94usize]));
        measures.push(registry.register_measure(&INFO[95usize]));
        measures.push(registry.register_measure(&INFO[97usize]));
        measures.push(registry.register_measure(&INFO[98usize]));
        measures.push(registry.register_measure(&INFO[99usize]));
        measures.push(registry.register_measure(&INFO[100usize]));
        measures.push(registry.register_measure(&INFO[103usize]));
        measures.push(registry.register_measure(&INFO[104usize]));
        measures.push(registry.register_measure(&INFO[105usize]));
        measures.push(registry.register_measure(&INFO[106usize]));
        measures.push(registry.register_measure(&INFO[109usize]));
        measures.push(registry.register_measure(&INFO[110usize]));
        measures.push(registry.register_measure(&INFO[111usize]));
        measures.push(registry.register_measure(&INFO[114usize]));
        measures.push(registry.register_measure(&INFO[115usize]));
        measures.push(registry.register_measure(&INFO[116usize]));
        measures.push(registry.register_measure(&INFO[122usize]));
        measures.push(registry.register_measure(&INFO[123usize]));
        measures.push(registry.register_measure(&INFO[125usize]));
        measures.push(registry.register_measure(&INFO[126usize]));
        measures.push(registry.register_measure(&INFO[128usize]));
        measures.push(registry.register_measure(&INFO[129usize]));
        measures.push(registry.register_measure(&INFO[131usize]));
        measures.push(registry.register_measure(&INFO[148usize]));
        measures.push(registry.register_measure(&INFO[150usize]));
        measures.push(registry.register_measure(&INFO[151usize]));
        measures.push(registry.register_measure(&INFO[152usize]));
        measures.push(registry.register_measure(&INFO[163usize]));
        measures.push(registry.register_measure(&INFO[166usize]));
        measures.push(registry.register_measure(&INFO[178usize]));
        measures.push(registry.register_measure(&INFO[179usize]));
        measures.push(registry.register_measure(&INFO[182usize]));
        measures.push(registry.register_measure(&INFO[208usize]));
        measures.push(registry.register_measure(&INFO[212usize]));
        measures.push(registry.register_measure(&INFO[214usize]));
        measures.push(registry.register_measure(&INFO[215usize]));
        measures.push(registry.register_measure(&INFO[216usize]));
        measures.push(registry.register_measure(&INFO[217usize]));
        measures.push(registry.register_measure(&INFO[218usize]));
        measures.push(registry.register_measure(&INFO[219usize]));
        measures.push(registry.register_measure(&INFO[220usize]));
        measures.push(registry.register_measure(&INFO[221usize]));
        measures.push(registry.register_measure(&INFO[222usize]));
        measures.push(registry.register_measure(&INFO[223usize]));
        measures.push(registry.register_measure(&INFO[224usize]));
        measures.push(registry.register_measure(&INFO[225usize]));
        measures.push(registry.register_measure(&INFO[226usize]));
        measures.push(registry.register_measure(&INFO[227usize]));
        measures.push(registry.register_measure(&INFO[228usize]));
        measures.push(registry.register_measure(&INFO[229usize]));
        timers.push(registry.register_timer(&INFO[5usize]));
        timers.push(registry.register_timer(&INFO[15usize]));
        timers.push(registry.register_timer(&INFO[21usize]));
        timers.push(registry.register_timer(&INFO[24usize]));
        timers.push(registry.register_timer(&INFO[26usize]));
        timers.push(registry.register_timer(&INFO[43usize]));
        timers.push(registry.register_timer(&INFO[46usize]));
        timers.push(registry.register_timer(&INFO[48usize]));
        timers.push(registry.register_timer(&INFO[57usize]));
        timers.push(registry.register_timer(&INFO[66usize]));
        timers.push(registry.register_timer(&INFO[72usize]));
        timers.push(registry.register_timer(&INFO[78usize]));
        timers.push(registry.register_timer(&INFO[93usize]));
        timers.push(registry.register_timer(&INFO[102usize]));
        timers.push(registry.register_timer(&INFO[108usize]));
        timers.push(registry.register_timer(&INFO[113usize]));
        timers.push(registry.register_timer(&INFO[119usize]));
        timers.push(registry.register_timer(&INFO[134usize]));
        {
            #[allow(unused_imports)]
            use api::*;
        }
        Self {
            counters: counters
                .try_into()
                .unwrap_or_else(|_| panic!("invalid len")),
            bool_counters: bool_counters
                .try_into()
                .unwrap_or_else(|_| panic!("invalid len")),
            nominal_counters: nominal_counters.into(),
            nominal_counter_offsets: nominal_counter_offsets
                .try_into()
                .unwrap_or_else(|_| panic!("invalid len")),
            measures: measures
                .try_into()
                .unwrap_or_else(|_| panic!("invalid len")),
            gauges: gauges.try_into().unwrap_or_else(|_| panic!("invalid len")),
            timers: timers.try_into().unwrap_or_else(|_| panic!("invalid len")),
            nominal_timers: nominal_timers.into(),
            nominal_timer_offsets: nominal_timer_offsets
                .try_into()
                .unwrap_or_else(|_| panic!("invalid len")),
            registry,
        }
    }
    #[doc = r" Returns all of the registered counters"]
    #[inline]
    pub fn counters(&self) -> impl Iterator<Item = (&'static Info, &R::Counter)> + '_ {
        self.counters
            .iter()
            .enumerate()
            .map(|(idx, entry)| match idx {
                0usize => (&INFO[0usize], entry),
                1usize => (&INFO[1usize], entry),
                2usize => (&INFO[7usize], entry),
                3usize => (&INFO[8usize], entry),
                4usize => (&INFO[12usize], entry),
                5usize => (&INFO[14usize], entry),
                6usize => (&INFO[17usize], entry),
                7usize => (&INFO[22usize], entry),
                8usize => (&INFO[25usize], entry),
                9usize => (&INFO[28usize], entry),
                10usize => (&INFO[29usize], entry),
                11usize => (&INFO[30usize], entry),
                12usize => (&INFO[32usize], entry),
                13usize => (&INFO[38usize], entry),
                14usize => (&INFO[40usize], entry),
                15usize => (&INFO[41usize], entry),
                16usize => (&INFO[42usize], entry),
                17usize => (&INFO[45usize], entry),
                18usize => (&INFO[47usize], entry),
                19usize => (&INFO[51usize], entry),
                20usize => (&INFO[56usize], entry),
                21usize => (&INFO[60usize], entry),
                22usize => (&INFO[65usize], entry),
                23usize => (&INFO[71usize], entry),
                24usize => (&INFO[76usize], entry),
                25usize => (&INFO[77usize], entry),
                26usize => (&INFO[81usize], entry),
                27usize => (&INFO[84usize], entry),
                28usize => (&INFO[87usize], entry),
                29usize => (&INFO[90usize], entry),
                30usize => (&INFO[92usize], entry),
                31usize => (&INFO[96usize], entry),
                32usize => (&INFO[101usize], entry),
                33usize => (&INFO[107usize], entry),
                34usize => (&INFO[112usize], entry),
                35usize => (&INFO[117usize], entry),
                36usize => (&INFO[118usize], entry),
                37usize => (&INFO[121usize], entry),
                38usize => (&INFO[124usize], entry),
                39usize => (&INFO[127usize], entry),
                40usize => (&INFO[130usize], entry),
                41usize => (&INFO[132usize], entry),
                42usize => (&INFO[135usize], entry),
                43usize => (&INFO[139usize], entry),
                44usize => (&INFO[141usize], entry),
                45usize => (&INFO[142usize], entry),
                46usize => (&INFO[147usize], entry),
                47usize => (&INFO[149usize], entry),
                48usize => (&INFO[153usize], entry),
                49usize => (&INFO[155usize], entry),
                50usize => (&INFO[157usize], entry),
                51usize => (&INFO[159usize], entry),
                52usize => (&INFO[161usize], entry),
                53usize => (&INFO[164usize], entry),
                54usize => (&INFO[167usize], entry),
                55usize => (&INFO[169usize], entry),
                56usize => (&INFO[171usize], entry),
                57usize => (&INFO[173usize], entry),
                58usize => (&INFO[175usize], entry),
                59usize => (&INFO[177usize], entry),
                60usize => (&INFO[180usize], entry),
                61usize => (&INFO[181usize], entry),
                62usize => (&INFO[183usize], entry),
                63usize => (&INFO[185usize], entry),
                64usize => (&INFO[187usize], entry),
                65usize => (&INFO[189usize], entry),
                66usize => (&INFO[191usize], entry),
                67usize => (&INFO[193usize], entry),
                68usize => (&INFO[195usize], entry),
                69usize => (&INFO[197usize], entry),
                70usize => (&INFO[199usize], entry),
                71usize => (&INFO[201usize], entry),
                72usize => (&INFO[203usize], entry),
                73usize => (&INFO[206usize], entry),
                74usize => (&INFO[209usize], entry),
                75usize => (&INFO[211usize], entry),
                76usize => (&INFO[213usize], entry),
                _ => unsafe { core::hint::unreachable_unchecked() },
            })
    }
    #[allow(dead_code)]
    #[inline(always)]
    fn count<T: Metric>(&self, info: usize, id: usize, value: T) {
        let info = &INFO[info];
        let counter = &self.counters[id];
        counter.record(info, value);
    }
    #[doc = r" Returns all of the registered bool counters"]
    #[inline]
    pub fn bool_counters(&self) -> impl Iterator<Item = (&'static Info, &R::BoolCounter)> + '_ {
        self.bool_counters
            .iter()
            .enumerate()
            .map(|(idx, entry)| match idx {
                0usize => (&INFO[19usize], entry),
                1usize => (&INFO[20usize], entry),
                2usize => (&INFO[34usize], entry),
                3usize => (&INFO[35usize], entry),
                4usize => (&INFO[36usize], entry),
                5usize => (&INFO[37usize], entry),
                6usize => (&INFO[80usize], entry),
                7usize => (&INFO[120usize], entry),
                8usize => (&INFO[133usize], entry),
                9usize => (&INFO[136usize], entry),
                10usize => (&INFO[145usize], entry),
                11usize => (&INFO[146usize], entry),
                12usize => (&INFO[205usize], entry),
                13usize => (&INFO[210usize], entry),
                _ => unsafe { core::hint::unreachable_unchecked() },
            })
    }
    #[allow(dead_code)]
    #[inline(always)]
    fn count_bool(&self, info: usize, id: usize, value: bool) {
        let info = &INFO[info];
        let counter = &self.bool_counters[id];
        counter.record(info, value);
    }
    #[doc = r" Returns all of the registered nominal counters"]
    #[inline]
    pub fn nominal_counters(
        &self,
    ) -> impl Iterator<Item = (&'static Info, &[R::NominalCounter], &[info::Variant])> + '_ {
        use api::*;
        self.nominal_counter_offsets
            .iter()
            .enumerate()
            .map(|(idx, entry)| match idx {
                0usize => {
                    let offset = *entry;
                    let variants = <AcceptorTcpStreamDropReason as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[13usize], entries, variants)
                }
                1usize => {
                    let offset = *entry;
                    let variants = <AcceptorPacketDropReason as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[23usize], entries, variants)
                }
                2usize => {
                    let offset = *entry;
                    let variants = <AcceptorPacketDropReason as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[39usize], entries, variants)
                }
                3usize => {
                    let offset = *entry;
                    let variants = <AcceptorStreamPruneReason as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[44usize], entries, variants)
                }
                4usize => {
                    let offset = *entry;
                    let variants = <MaybeBoolCounter as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[137usize], entries, variants)
                }
                5usize => {
                    let offset = *entry;
                    let variants = <MaybeBoolCounter as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[138usize], entries, variants)
                }
                6usize => {
                    let offset = *entry;
                    let variants = <StreamTcpConnectErrorReason as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[140usize], entries, variants)
                }
                7usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[143usize], entries, variants)
                }
                8usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[144usize], entries, variants)
                }
                9usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[154usize], entries, variants)
                }
                10usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[156usize], entries, variants)
                }
                11usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[158usize], entries, variants)
                }
                12usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[160usize], entries, variants)
                }
                13usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[162usize], entries, variants)
                }
                14usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[165usize], entries, variants)
                }
                15usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[168usize], entries, variants)
                }
                16usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[170usize], entries, variants)
                }
                17usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[172usize], entries, variants)
                }
                18usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[174usize], entries, variants)
                }
                19usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[176usize], entries, variants)
                }
                20usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[184usize], entries, variants)
                }
                21usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[186usize], entries, variants)
                }
                22usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[188usize], entries, variants)
                }
                23usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[190usize], entries, variants)
                }
                24usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[192usize], entries, variants)
                }
                25usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[194usize], entries, variants)
                }
                26usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[196usize], entries, variants)
                }
                27usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[198usize], entries, variants)
                }
                28usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[200usize], entries, variants)
                }
                29usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[202usize], entries, variants)
                }
                30usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[204usize], entries, variants)
                }
                31usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[207usize], entries, variants)
                }
                _ => unsafe { core::hint::unreachable_unchecked() },
            })
    }
    #[allow(dead_code)]
    #[inline(always)]
    fn count_nominal<T: AsVariant>(&self, info: usize, id: usize, value: &T) {
        let info = &INFO[info];
        let idx = self.nominal_counter_offsets[id] + value.variant_idx();
        let counter = &self.nominal_counters[idx];
        counter.record(info, value.as_variant(), 1usize);
    }
    #[doc = r" Returns all of the registered measures"]
    #[inline]
    pub fn measures(&self) -> impl Iterator<Item = (&'static Info, &R::Measure)> + '_ {
        self.measures
            .iter()
            .enumerate()
            .map(|(idx, entry)| match idx {
                0usize => (&INFO[2usize], entry),
                1usize => (&INFO[3usize], entry),
                2usize => (&INFO[4usize], entry),
                3usize => (&INFO[6usize], entry),
                4usize => (&INFO[9usize], entry),
                5usize => (&INFO[10usize], entry),
                6usize => (&INFO[11usize], entry),
                7usize => (&INFO[16usize], entry),
                8usize => (&INFO[18usize], entry),
                9usize => (&INFO[27usize], entry),
                10usize => (&INFO[31usize], entry),
                11usize => (&INFO[33usize], entry),
                12usize => (&INFO[49usize], entry),
                13usize => (&INFO[50usize], entry),
                14usize => (&INFO[52usize], entry),
                15usize => (&INFO[53usize], entry),
                16usize => (&INFO[54usize], entry),
                17usize => (&INFO[55usize], entry),
                18usize => (&INFO[58usize], entry),
                19usize => (&INFO[59usize], entry),
                20usize => (&INFO[61usize], entry),
                21usize => (&INFO[62usize], entry),
                22usize => (&INFO[63usize], entry),
                23usize => (&INFO[64usize], entry),
                24usize => (&INFO[67usize], entry),
                25usize => (&INFO[68usize], entry),
                26usize => (&INFO[69usize], entry),
                27usize => (&INFO[70usize], entry),
                28usize => (&INFO[73usize], entry),
                29usize => (&INFO[74usize], entry),
                30usize => (&INFO[75usize], entry),
                31usize => (&INFO[79usize], entry),
                32usize => (&INFO[82usize], entry),
                33usize => (&INFO[83usize], entry),
                34usize => (&INFO[85usize], entry),
                35usize => (&INFO[86usize], entry),
                36usize => (&INFO[88usize], entry),
                37usize => (&INFO[89usize], entry),
                38usize => (&INFO[91usize], entry),
                39usize => (&INFO[94usize], entry),
                40usize => (&INFO[95usize], entry),
                41usize => (&INFO[97usize], entry),
                42usize => (&INFO[98usize], entry),
                43usize => (&INFO[99usize], entry),
                44usize => (&INFO[100usize], entry),
                45usize => (&INFO[103usize], entry),
                46usize => (&INFO[104usize], entry),
                47usize => (&INFO[105usize], entry),
                48usize => (&INFO[106usize], entry),
                49usize => (&INFO[109usize], entry),
                50usize => (&INFO[110usize], entry),
                51usize => (&INFO[111usize], entry),
                52usize => (&INFO[114usize], entry),
                53usize => (&INFO[115usize], entry),
                54usize => (&INFO[116usize], entry),
                55usize => (&INFO[122usize], entry),
                56usize => (&INFO[123usize], entry),
                57usize => (&INFO[125usize], entry),
                58usize => (&INFO[126usize], entry),
                59usize => (&INFO[128usize], entry),
                60usize => (&INFO[129usize], entry),
                61usize => (&INFO[131usize], entry),
                62usize => (&INFO[148usize], entry),
                63usize => (&INFO[150usize], entry),
                64usize => (&INFO[151usize], entry),
                65usize => (&INFO[152usize], entry),
                66usize => (&INFO[163usize], entry),
                67usize => (&INFO[166usize], entry),
                68usize => (&INFO[178usize], entry),
                69usize => (&INFO[179usize], entry),
                70usize => (&INFO[182usize], entry),
                71usize => (&INFO[208usize], entry),
                72usize => (&INFO[212usize], entry),
                73usize => (&INFO[214usize], entry),
                74usize => (&INFO[215usize], entry),
                75usize => (&INFO[216usize], entry),
                76usize => (&INFO[217usize], entry),
                77usize => (&INFO[218usize], entry),
                78usize => (&INFO[219usize], entry),
                79usize => (&INFO[220usize], entry),
                80usize => (&INFO[221usize], entry),
                81usize => (&INFO[222usize], entry),
                82usize => (&INFO[223usize], entry),
                83usize => (&INFO[224usize], entry),
                84usize => (&INFO[225usize], entry),
                85usize => (&INFO[226usize], entry),
                86usize => (&INFO[227usize], entry),
                87usize => (&INFO[228usize], entry),
                88usize => (&INFO[229usize], entry),
                _ => unsafe { core::hint::unreachable_unchecked() },
            })
    }
    #[allow(dead_code)]
    #[inline(always)]
    fn measure<T: Metric>(&self, info: usize, id: usize, value: T) {
        let info = &INFO[info];
        let measure = &self.measures[id];
        measure.record(info, value);
    }
    #[doc = r" Returns all of the registered gauges"]
    #[inline]
    pub fn gauges(&self) -> impl Iterator<Item = (&'static Info, &R::Gauge)> + '_ {
        core::iter::empty()
    }
    #[allow(dead_code)]
    #[inline(always)]
    fn gauge<T: Metric>(&self, info: usize, id: usize, value: T) {
        let info = &INFO[info];
        let gauge = &self.gauges[id];
        gauge.record(info, value);
    }
    #[doc = r" Returns all of the registered timers"]
    #[inline]
    pub fn timers(&self) -> impl Iterator<Item = (&'static Info, &R::Timer)> + '_ {
        self.timers
            .iter()
            .enumerate()
            .map(|(idx, entry)| match idx {
                0usize => (&INFO[5usize], entry),
                1usize => (&INFO[15usize], entry),
                2usize => (&INFO[21usize], entry),
                3usize => (&INFO[24usize], entry),
                4usize => (&INFO[26usize], entry),
                5usize => (&INFO[43usize], entry),
                6usize => (&INFO[46usize], entry),
                7usize => (&INFO[48usize], entry),
                8usize => (&INFO[57usize], entry),
                9usize => (&INFO[66usize], entry),
                10usize => (&INFO[72usize], entry),
                11usize => (&INFO[78usize], entry),
                12usize => (&INFO[93usize], entry),
                13usize => (&INFO[102usize], entry),
                14usize => (&INFO[108usize], entry),
                15usize => (&INFO[113usize], entry),
                16usize => (&INFO[119usize], entry),
                17usize => (&INFO[134usize], entry),
                _ => unsafe { core::hint::unreachable_unchecked() },
            })
    }
    #[allow(dead_code)]
    #[inline(always)]
    fn time(&self, info: usize, id: usize, value: core::time::Duration) {
        let info = &INFO[info];
        let timer = &self.timers[id];
        timer.record(info, value);
    }
    #[allow(dead_code)]
    #[inline(always)]
    fn time_nominal<T: AsVariant>(
        &self,
        info: usize,
        id: usize,
        value: &T,
        duration: core::time::Duration,
    ) {
        let info = &INFO[info];
        let idx = self.nominal_timer_offsets[id] + value.variant_idx();
        let counter = &self.nominal_timers[idx];
        counter.record(info, value.as_variant(), duration);
    }
}
impl<R: Registry> event::Subscriber for Subscriber<R> {
    type ConnectionContext = ConnectionContext;
    fn create_connection_context(
        &self,
        meta: &api::ConnectionMeta,
        _info: &api::ConnectionInfo,
    ) -> Self::ConnectionContext {
        Self::ConnectionContext {
            start_time: meta.timestamp,
            ctr_12: AtomicU64::new(0),
            ctr_15: AtomicU64::new(0),
            ctr_17: AtomicU64::new(0),
            ctr_18: AtomicU64::new(0),
            ctr_21: AtomicU64::new(0),
            ctr_23: AtomicU64::new(0),
            ctr_24: AtomicU64::new(0),
            ctr_27: AtomicU64::new(0),
            ctr_30: AtomicU64::new(0),
            ctr_32: AtomicU64::new(0),
            ctr_35: AtomicU64::new(0),
            ctr_36: AtomicU64::new(0),
            ctr_39: AtomicU64::new(0),
            ctr_42: AtomicU64::new(0),
            ctr_44: AtomicU64::new(0),
            ctr_45: AtomicU64::new(0),
            ctr_48: AtomicU64::new(0),
            ctr_51: AtomicU64::new(0),
            ctr_54: AtomicU64::new(0),
            ctr_55: AtomicU64::new(0),
            ctr_58: AtomicU64::new(0),
            ctr_59: AtomicU64::new(0),
        }
    }
    #[inline]
    fn on_acceptor_tcp_started(&self, meta: &api::EndpointMeta, event: &api::AcceptorTcpStarted) {
        #[allow(unused_imports)]
        use api::*;
        self.count(0usize, 0usize, 1usize);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_tcp_loop_iteration_completed(
        &self,
        meta: &api::EndpointMeta,
        event: &api::AcceptorTcpLoopIterationCompleted,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(1usize, 1usize, 1usize);
        self.measure(2usize, 0usize, event.pending_streams);
        self.measure(3usize, 1usize, event.slots_idle);
        self.measure(4usize, 2usize, event.slot_utilization);
        self.time(5usize, 0usize, event.processing_duration);
        self.measure(6usize, 3usize, event.max_sojourn_time);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_tcp_fresh_enqueued(
        &self,
        meta: &api::EndpointMeta,
        event: &api::AcceptorTcpFreshEnqueued,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(7usize, 2usize, 1usize);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_tcp_fresh_batch_completed(
        &self,
        meta: &api::EndpointMeta,
        event: &api::AcceptorTcpFreshBatchCompleted,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(8usize, 3usize, 1usize);
        self.measure(9usize, 4usize, event.enqueued);
        self.measure(10usize, 5usize, event.dropped);
        self.measure(11usize, 6usize, event.errored);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_tcp_stream_dropped(
        &self,
        meta: &api::EndpointMeta,
        event: &api::AcceptorTcpStreamDropped,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(12usize, 4usize, 1usize);
        self.count_nominal(13usize, 0usize, &event.reason);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_tcp_stream_replaced(
        &self,
        meta: &api::EndpointMeta,
        event: &api::AcceptorTcpStreamReplaced,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(14usize, 5usize, 1usize);
        self.time(15usize, 1usize, event.sojourn_time);
        self.measure(16usize, 7usize, event.buffer_len);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_tcp_packet_received(
        &self,
        meta: &api::EndpointMeta,
        event: &api::AcceptorTcpPacketReceived,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(17usize, 6usize, 1usize);
        self.measure(18usize, 8usize, event.payload_len);
        self.count_bool(19usize, 0usize, event.is_fin);
        self.count_bool(20usize, 1usize, event.is_fin_known);
        self.time(21usize, 2usize, event.sojourn_time);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_tcp_packet_dropped(
        &self,
        meta: &api::EndpointMeta,
        event: &api::AcceptorTcpPacketDropped,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(22usize, 7usize, 1usize);
        self.count_nominal(23usize, 1usize, &event.reason);
        self.time(24usize, 3usize, event.sojourn_time);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_tcp_stream_enqueued(
        &self,
        meta: &api::EndpointMeta,
        event: &api::AcceptorTcpStreamEnqueued,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(25usize, 8usize, 1usize);
        self.time(26usize, 4usize, event.sojourn_time);
        self.measure(27usize, 9usize, event.blocked_count);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_tcp_io_error(&self, meta: &api::EndpointMeta, event: &api::AcceptorTcpIoError) {
        #[allow(unused_imports)]
        use api::*;
        self.count(28usize, 9usize, 1usize);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_udp_started(&self, meta: &api::EndpointMeta, event: &api::AcceptorUdpStarted) {
        #[allow(unused_imports)]
        use api::*;
        self.count(29usize, 10usize, 1usize);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_udp_datagram_received(
        &self,
        meta: &api::EndpointMeta,
        event: &api::AcceptorUdpDatagramReceived,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(30usize, 11usize, 1usize);
        self.measure(31usize, 10usize, event.len);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_udp_packet_received(
        &self,
        meta: &api::EndpointMeta,
        event: &api::AcceptorUdpPacketReceived,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(32usize, 12usize, 1usize);
        self.measure(33usize, 11usize, event.payload_len);
        self.count_bool(34usize, 2usize, event.is_zero_offset);
        self.count_bool(35usize, 3usize, event.is_retransmission);
        self.count_bool(36usize, 4usize, event.is_fin);
        self.count_bool(37usize, 5usize, event.is_fin_known);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_udp_packet_dropped(
        &self,
        meta: &api::EndpointMeta,
        event: &api::AcceptorUdpPacketDropped,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(38usize, 13usize, 1usize);
        self.count_nominal(39usize, 2usize, &event.reason);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_udp_stream_enqueued(
        &self,
        meta: &api::EndpointMeta,
        event: &api::AcceptorUdpStreamEnqueued,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(40usize, 14usize, 1usize);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_udp_io_error(&self, meta: &api::EndpointMeta, event: &api::AcceptorUdpIoError) {
        #[allow(unused_imports)]
        use api::*;
        self.count(41usize, 15usize, 1usize);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_stream_pruned(
        &self,
        meta: &api::EndpointMeta,
        event: &api::AcceptorStreamPruned,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(42usize, 16usize, 1usize);
        self.time(43usize, 5usize, event.sojourn_time);
        self.count_nominal(44usize, 3usize, &event.reason);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_stream_dequeued(
        &self,
        meta: &api::EndpointMeta,
        event: &api::AcceptorStreamDequeued,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(45usize, 17usize, 1usize);
        self.time(46usize, 6usize, event.sojourn_time);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_stream_write_flushed(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamWriteFlushed,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(47usize, 18usize, 1usize);
        self.time(
            48usize,
            7usize,
            meta.timestamp.saturating_duration_since(context.start_time),
        );
        context.ctr_12.fetch_add(1, Ordering::Relaxed);
        self.measure(50usize, 13usize, event.provided_len);
        self.count(51usize, 19usize, event.committed_len);
        self.measure(52usize, 14usize, event.committed_len);
        context
            .ctr_15
            .fetch_add(event.committed_len.as_u64(), Ordering::Relaxed);
        self.measure(54usize, 16usize, event.processing_duration);
        context
            .ctr_17
            .fetch_add(event.processing_duration.as_u64(), Ordering::Relaxed);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_stream_write_fin_flushed(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamWriteFinFlushed,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(56usize, 20usize, 1usize);
        self.time(
            57usize,
            8usize,
            meta.timestamp.saturating_duration_since(context.start_time),
        );
        context.ctr_18.fetch_add(1, Ordering::Relaxed);
        self.measure(59usize, 19usize, event.provided_len);
        self.count(60usize, 21usize, event.committed_len);
        self.measure(61usize, 20usize, event.committed_len);
        context
            .ctr_21
            .fetch_add(event.committed_len.as_u64(), Ordering::Relaxed);
        self.measure(63usize, 22usize, event.processing_duration);
        context
            .ctr_23
            .fetch_add(event.processing_duration.as_u64(), Ordering::Relaxed);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_stream_write_blocked(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamWriteBlocked,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(65usize, 22usize, 1usize);
        self.time(
            66usize,
            9usize,
            meta.timestamp.saturating_duration_since(context.start_time),
        );
        context.ctr_24.fetch_add(1, Ordering::Relaxed);
        self.measure(68usize, 25usize, event.provided_len);
        self.measure(69usize, 26usize, event.processing_duration);
        context
            .ctr_27
            .fetch_add(event.processing_duration.as_u64(), Ordering::Relaxed);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_stream_write_errored(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamWriteErrored,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(71usize, 23usize, 1usize);
        self.time(
            72usize,
            10usize,
            meta.timestamp.saturating_duration_since(context.start_time),
        );
        self.measure(73usize, 28usize, event.provided_len);
        self.measure(74usize, 29usize, event.processing_duration);
        context
            .ctr_30
            .fetch_add(event.processing_duration.as_u64(), Ordering::Relaxed);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_stream_write_key_updated(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamWriteKeyUpdated,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(76usize, 24usize, 1usize);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_stream_write_shutdown(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamWriteShutdown,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(77usize, 25usize, 1usize);
        self.time(
            78usize,
            11usize,
            meta.timestamp.saturating_duration_since(context.start_time),
        );
        self.measure(79usize, 31usize, event.buffer_len);
        self.count_bool(80usize, 6usize, event.background);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_stream_write_socket_flushed(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamWriteSocketFlushed,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(81usize, 26usize, 1usize);
        context.ctr_32.fetch_add(1, Ordering::Relaxed);
        self.measure(83usize, 33usize, event.provided_len);
        self.count(84usize, 27usize, event.committed_len);
        self.measure(85usize, 34usize, event.committed_len);
        context
            .ctr_35
            .fetch_add(event.committed_len.as_u64(), Ordering::Relaxed);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_stream_write_socket_blocked(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamWriteSocketBlocked,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(87usize, 28usize, 1usize);
        context.ctr_36.fetch_add(1, Ordering::Relaxed);
        self.measure(89usize, 37usize, event.provided_len);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_stream_write_socket_errored(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamWriteSocketErrored,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(90usize, 29usize, 1usize);
        self.measure(91usize, 38usize, event.provided_len);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_stream_read_flushed(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamReadFlushed,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(92usize, 30usize, 1usize);
        self.time(
            93usize,
            12usize,
            meta.timestamp.saturating_duration_since(context.start_time),
        );
        context.ctr_39.fetch_add(1, Ordering::Relaxed);
        self.measure(95usize, 40usize, event.capacity);
        self.count(96usize, 31usize, event.committed_len);
        self.measure(97usize, 41usize, event.committed_len);
        context
            .ctr_42
            .fetch_add(event.committed_len.as_u64(), Ordering::Relaxed);
        self.measure(99usize, 43usize, event.processing_duration);
        context
            .ctr_44
            .fetch_add(event.processing_duration.as_u64(), Ordering::Relaxed);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_stream_read_fin_flushed(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamReadFinFlushed,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(101usize, 32usize, 1usize);
        self.time(
            102usize,
            13usize,
            meta.timestamp.saturating_duration_since(context.start_time),
        );
        context.ctr_45.fetch_add(1, Ordering::Relaxed);
        self.measure(104usize, 46usize, event.capacity);
        self.measure(105usize, 47usize, event.processing_duration);
        context
            .ctr_48
            .fetch_add(event.processing_duration.as_u64(), Ordering::Relaxed);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_stream_read_blocked(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamReadBlocked,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(107usize, 33usize, 1usize);
        self.time(
            108usize,
            14usize,
            meta.timestamp.saturating_duration_since(context.start_time),
        );
        self.measure(109usize, 49usize, event.capacity);
        self.measure(110usize, 50usize, event.processing_duration);
        context
            .ctr_51
            .fetch_add(event.processing_duration.as_u64(), Ordering::Relaxed);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_stream_read_errored(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamReadErrored,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(112usize, 34usize, 1usize);
        self.time(
            113usize,
            15usize,
            meta.timestamp.saturating_duration_since(context.start_time),
        );
        self.measure(114usize, 52usize, event.capacity);
        self.measure(115usize, 53usize, event.processing_duration);
        context
            .ctr_54
            .fetch_add(event.processing_duration.as_u64(), Ordering::Relaxed);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_stream_read_key_updated(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamReadKeyUpdated,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(117usize, 35usize, 1usize);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_stream_read_shutdown(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamReadShutdown,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(118usize, 36usize, 1usize);
        self.time(
            119usize,
            16usize,
            meta.timestamp.saturating_duration_since(context.start_time),
        );
        self.count_bool(120usize, 7usize, event.background);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_stream_read_socket_flushed(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamReadSocketFlushed,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(121usize, 37usize, 1usize);
        context.ctr_55.fetch_add(1, Ordering::Relaxed);
        self.measure(123usize, 56usize, event.capacity);
        self.count(124usize, 38usize, event.committed_len);
        self.measure(125usize, 57usize, event.committed_len);
        context
            .ctr_58
            .fetch_add(event.committed_len.as_u64(), Ordering::Relaxed);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_stream_read_socket_blocked(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamReadSocketBlocked,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(127usize, 39usize, 1usize);
        context.ctr_59.fetch_add(1, Ordering::Relaxed);
        self.measure(129usize, 60usize, event.capacity);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_stream_read_socket_errored(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamReadSocketErrored,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(130usize, 40usize, 1usize);
        self.measure(131usize, 61usize, event.capacity);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_stream_tcp_connect(&self, meta: &api::EndpointMeta, event: &api::StreamTcpConnect) {
        #[allow(unused_imports)]
        use api::*;
        self.count(132usize, 41usize, 1usize);
        self.count_bool(133usize, 8usize, event.error);
        self.time(134usize, 17usize, event.latency);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_stream_connect(&self, meta: &api::EndpointMeta, event: &api::StreamConnect) {
        #[allow(unused_imports)]
        use api::*;
        self.count(135usize, 42usize, 1usize);
        self.count_bool(136usize, 9usize, event.error);
        self.count_nominal(137usize, 4usize, &event.tcp_success);
        self.count_nominal(138usize, 5usize, &event.handshake_success);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_stream_connect_error(&self, meta: &api::EndpointMeta, event: &api::StreamConnectError) {
        #[allow(unused_imports)]
        use api::*;
        self.count(139usize, 43usize, 1usize);
        self.count_nominal(140usize, 6usize, &event.reason);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_connection_closed(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::ConnectionClosed,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(141usize, 44usize, 1usize);
        self.measure(49usize, 12usize, context.ctr_12.load(Ordering::Relaxed));
        self.measure(53usize, 15usize, context.ctr_15.load(Ordering::Relaxed));
        self.measure(
            55usize,
            17usize,
            core::time::Duration::from_micros(context.ctr_17.load(Ordering::Relaxed)),
        );
        self.measure(58usize, 18usize, context.ctr_18.load(Ordering::Relaxed));
        self.measure(62usize, 21usize, context.ctr_21.load(Ordering::Relaxed));
        self.measure(
            64usize,
            23usize,
            core::time::Duration::from_micros(context.ctr_23.load(Ordering::Relaxed)),
        );
        self.measure(67usize, 24usize, context.ctr_24.load(Ordering::Relaxed));
        self.measure(
            70usize,
            27usize,
            core::time::Duration::from_micros(context.ctr_27.load(Ordering::Relaxed)),
        );
        self.measure(
            75usize,
            30usize,
            core::time::Duration::from_micros(context.ctr_30.load(Ordering::Relaxed)),
        );
        self.measure(82usize, 32usize, context.ctr_32.load(Ordering::Relaxed));
        self.measure(86usize, 35usize, context.ctr_35.load(Ordering::Relaxed));
        self.measure(88usize, 36usize, context.ctr_36.load(Ordering::Relaxed));
        self.measure(94usize, 39usize, context.ctr_39.load(Ordering::Relaxed));
        self.measure(98usize, 42usize, context.ctr_42.load(Ordering::Relaxed));
        self.measure(
            100usize,
            44usize,
            core::time::Duration::from_micros(context.ctr_44.load(Ordering::Relaxed)),
        );
        self.measure(103usize, 45usize, context.ctr_45.load(Ordering::Relaxed));
        self.measure(
            106usize,
            48usize,
            core::time::Duration::from_micros(context.ctr_48.load(Ordering::Relaxed)),
        );
        self.measure(
            111usize,
            51usize,
            core::time::Duration::from_micros(context.ctr_51.load(Ordering::Relaxed)),
        );
        self.measure(
            116usize,
            54usize,
            core::time::Duration::from_micros(context.ctr_54.load(Ordering::Relaxed)),
        );
        self.measure(122usize, 55usize, context.ctr_55.load(Ordering::Relaxed));
        self.measure(126usize, 58usize, context.ctr_58.load(Ordering::Relaxed));
        self.measure(128usize, 59usize, context.ctr_59.load(Ordering::Relaxed));
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_endpoint_initialized(&self, meta: &api::EndpointMeta, event: &api::EndpointInitialized) {
        #[allow(unused_imports)]
        use api::*;
        self.count(142usize, 45usize, 1usize);
        self.count_nominal(143usize, 7usize, &event.acceptor_addr);
        self.count_nominal(144usize, 8usize, &event.handshake_addr);
        self.count_bool(145usize, 10usize, event.tcp);
        self.count_bool(146usize, 11usize, event.udp);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_path_secret_map_initialized(
        &self,
        meta: &api::EndpointMeta,
        event: &api::PathSecretMapInitialized,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(147usize, 46usize, 1usize);
        self.measure(148usize, 62usize, event.capacity);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_path_secret_map_uninitialized(
        &self,
        meta: &api::EndpointMeta,
        event: &api::PathSecretMapUninitialized,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(149usize, 47usize, 1usize);
        self.measure(150usize, 63usize, event.capacity);
        self.measure(151usize, 64usize, event.entries);
        self.measure(152usize, 65usize, event.lifetime);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_path_secret_map_background_handshake_requested(
        &self,
        meta: &api::EndpointMeta,
        event: &api::PathSecretMapBackgroundHandshakeRequested,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(153usize, 48usize, 1usize);
        self.count_nominal(154usize, 9usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_path_secret_map_entry_inserted(
        &self,
        meta: &api::EndpointMeta,
        event: &api::PathSecretMapEntryInserted,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(155usize, 49usize, 1usize);
        self.count_nominal(156usize, 10usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_path_secret_map_entry_ready(
        &self,
        meta: &api::EndpointMeta,
        event: &api::PathSecretMapEntryReady,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(157usize, 50usize, 1usize);
        self.count_nominal(158usize, 11usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_path_secret_map_entry_replaced(
        &self,
        meta: &api::EndpointMeta,
        event: &api::PathSecretMapEntryReplaced,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(159usize, 51usize, 1usize);
        self.count_nominal(160usize, 12usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_path_secret_map_id_entry_evicted(
        &self,
        meta: &api::EndpointMeta,
        event: &api::PathSecretMapIdEntryEvicted,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(161usize, 52usize, 1usize);
        self.count_nominal(162usize, 13usize, &event.peer_address);
        self.measure(163usize, 66usize, event.age);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_path_secret_map_address_entry_evicted(
        &self,
        meta: &api::EndpointMeta,
        event: &api::PathSecretMapAddressEntryEvicted,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(164usize, 53usize, 1usize);
        self.count_nominal(165usize, 14usize, &event.peer_address);
        self.measure(166usize, 67usize, event.age);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_unknown_path_secret_packet_sent(
        &self,
        meta: &api::EndpointMeta,
        event: &api::UnknownPathSecretPacketSent,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(167usize, 54usize, 1usize);
        self.count_nominal(168usize, 15usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_unknown_path_secret_packet_received(
        &self,
        meta: &api::EndpointMeta,
        event: &api::UnknownPathSecretPacketReceived,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(169usize, 55usize, 1usize);
        self.count_nominal(170usize, 16usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_unknown_path_secret_packet_accepted(
        &self,
        meta: &api::EndpointMeta,
        event: &api::UnknownPathSecretPacketAccepted,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(171usize, 56usize, 1usize);
        self.count_nominal(172usize, 17usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_unknown_path_secret_packet_rejected(
        &self,
        meta: &api::EndpointMeta,
        event: &api::UnknownPathSecretPacketRejected,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(173usize, 57usize, 1usize);
        self.count_nominal(174usize, 18usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_unknown_path_secret_packet_dropped(
        &self,
        meta: &api::EndpointMeta,
        event: &api::UnknownPathSecretPacketDropped,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(175usize, 58usize, 1usize);
        self.count_nominal(176usize, 19usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_key_accepted(&self, meta: &api::EndpointMeta, event: &api::KeyAccepted) {
        #[allow(unused_imports)]
        use api::*;
        self.count(177usize, 59usize, 1usize);
        self.measure(178usize, 68usize, event.gap);
        self.measure(179usize, 69usize, event.forward_shift);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_replay_definitely_detected(
        &self,
        meta: &api::EndpointMeta,
        event: &api::ReplayDefinitelyDetected,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(180usize, 60usize, 1usize);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_replay_potentially_detected(
        &self,
        meta: &api::EndpointMeta,
        event: &api::ReplayPotentiallyDetected,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(181usize, 61usize, 1usize);
        self.measure(182usize, 70usize, event.gap);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_replay_detected_packet_sent(
        &self,
        meta: &api::EndpointMeta,
        event: &api::ReplayDetectedPacketSent,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(183usize, 62usize, 1usize);
        self.count_nominal(184usize, 20usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_replay_detected_packet_received(
        &self,
        meta: &api::EndpointMeta,
        event: &api::ReplayDetectedPacketReceived,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(185usize, 63usize, 1usize);
        self.count_nominal(186usize, 21usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_replay_detected_packet_accepted(
        &self,
        meta: &api::EndpointMeta,
        event: &api::ReplayDetectedPacketAccepted,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(187usize, 64usize, 1usize);
        self.count_nominal(188usize, 22usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_replay_detected_packet_rejected(
        &self,
        meta: &api::EndpointMeta,
        event: &api::ReplayDetectedPacketRejected,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(189usize, 65usize, 1usize);
        self.count_nominal(190usize, 23usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_replay_detected_packet_dropped(
        &self,
        meta: &api::EndpointMeta,
        event: &api::ReplayDetectedPacketDropped,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(191usize, 66usize, 1usize);
        self.count_nominal(192usize, 24usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_stale_key_packet_sent(&self, meta: &api::EndpointMeta, event: &api::StaleKeyPacketSent) {
        #[allow(unused_imports)]
        use api::*;
        self.count(193usize, 67usize, 1usize);
        self.count_nominal(194usize, 25usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_stale_key_packet_received(
        &self,
        meta: &api::EndpointMeta,
        event: &api::StaleKeyPacketReceived,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(195usize, 68usize, 1usize);
        self.count_nominal(196usize, 26usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_stale_key_packet_accepted(
        &self,
        meta: &api::EndpointMeta,
        event: &api::StaleKeyPacketAccepted,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(197usize, 69usize, 1usize);
        self.count_nominal(198usize, 27usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_stale_key_packet_rejected(
        &self,
        meta: &api::EndpointMeta,
        event: &api::StaleKeyPacketRejected,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(199usize, 70usize, 1usize);
        self.count_nominal(200usize, 28usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_stale_key_packet_dropped(
        &self,
        meta: &api::EndpointMeta,
        event: &api::StaleKeyPacketDropped,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(201usize, 71usize, 1usize);
        self.count_nominal(202usize, 29usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_path_secret_map_address_cache_accessed(
        &self,
        meta: &api::EndpointMeta,
        event: &api::PathSecretMapAddressCacheAccessed,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(203usize, 72usize, 1usize);
        self.count_nominal(204usize, 30usize, &event.peer_address);
        self.count_bool(205usize, 12usize, event.hit);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_path_secret_map_address_cache_accessed_hit(
        &self,
        meta: &api::EndpointMeta,
        event: &api::PathSecretMapAddressCacheAccessedHit,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(206usize, 73usize, 1usize);
        self.count_nominal(207usize, 31usize, &event.peer_address);
        self.measure(208usize, 71usize, event.age);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_path_secret_map_id_cache_accessed(
        &self,
        meta: &api::EndpointMeta,
        event: &api::PathSecretMapIdCacheAccessed,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(209usize, 74usize, 1usize);
        self.count_bool(210usize, 13usize, event.hit);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_path_secret_map_id_cache_accessed_hit(
        &self,
        meta: &api::EndpointMeta,
        event: &api::PathSecretMapIdCacheAccessedHit,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(211usize, 75usize, 1usize);
        self.measure(212usize, 72usize, event.age);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_path_secret_map_cleaner_cycled(
        &self,
        meta: &api::EndpointMeta,
        event: &api::PathSecretMapCleanerCycled,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(213usize, 76usize, 1usize);
        self.measure(214usize, 73usize, event.id_entries);
        self.measure(215usize, 74usize, event.id_entries_retired);
        self.measure(216usize, 75usize, event.id_entries_active);
        self.measure(217usize, 76usize, event.id_entries_active_utilization);
        self.measure(218usize, 77usize, event.id_entries_utilization);
        self.measure(219usize, 78usize, event.id_entries_initial_utilization);
        self.measure(220usize, 79usize, event.address_entries);
        self.measure(221usize, 80usize, event.address_entries_active);
        self.measure(222usize, 81usize, event.address_entries_active_utilization);
        self.measure(223usize, 82usize, event.address_entries_retired);
        self.measure(224usize, 83usize, event.address_entries_utilization);
        self.measure(225usize, 84usize, event.address_entries_initial_utilization);
        self.measure(226usize, 85usize, event.handshake_requests);
        self.measure(227usize, 86usize, event.handshake_requests_retired);
        self.measure(228usize, 87usize, event.handshake_lock_duration);
        self.measure(229usize, 88usize, event.duration);
        let _ = event;
        let _ = meta;
    }
}
