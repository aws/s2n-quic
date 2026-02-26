// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// DO NOT MODIFY THIS FILE
// This file was generated with the `s2n-events` crate and any required
// changes should be made there.

use crate::event::{
    self, api,
    metrics::aggregate::{
        info::{self, Str},
        AsVariant, BoolRecorder, Info, Metric, NominalRecorder, Recorder, Registry, Units,
    },
};
use core::sync::atomic::{AtomicU64, Ordering};
static INFO: &[Info; 310usize] = &[
    info::Builder {
        id: 0usize,
        name: Str::new("acceptor_tcp_started\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 1usize,
        name: Str::new("acceptor_tcp_started.backlog\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 2usize,
        name: Str::new("acceptor_tcp_loop_iteration_completed\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 3usize,
        name: Str::new("acceptor_tcp_loop_iteration_completed.pending_streams\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 4usize,
        name: Str::new("acceptor_tcp_loop_iteration_completed.slots_idle\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 5usize,
        name: Str::new("acceptor_tcp_loop_iteration_completed.slot_utilization\0"),
        units: Units::Percent,
    }
    .build(),
    info::Builder {
        id: 6usize,
        name: Str::new("acceptor_tcp_loop_iteration_completed.processing_duration\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 7usize,
        name: Str::new("acceptor_tcp_loop_iteration_completed.max_sojourn_time\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 8usize,
        name: Str::new("acceptor_tcp_fresh_enqueued\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 9usize,
        name: Str::new("acceptor_tcp_fresh_batch_completed\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 10usize,
        name: Str::new("acceptor_tcp_fresh_batch_completed.enqueued\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 11usize,
        name: Str::new("acceptor_tcp_fresh_batch_completed.dropped\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 12usize,
        name: Str::new("acceptor_tcp_fresh_batch_completed.errored\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 13usize,
        name: Str::new("acceptor_tcp_stream_dropped\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 14usize,
        name: Str::new("acceptor_tcp_stream_dropped.reason\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 15usize,
        name: Str::new("acceptor_tcp_stream_replaced\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 16usize,
        name: Str::new("acceptor_tcp_stream_replaced.sojourn_time\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 17usize,
        name: Str::new("acceptor_tcp_stream_replaced.buffer_len\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 18usize,
        name: Str::new("acceptor_tcp_packet_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 19usize,
        name: Str::new("acceptor_tcp_packet_received.payload_len\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 20usize,
        name: Str::new("acceptor_tcp_packet_received.is_fin\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 21usize,
        name: Str::new("acceptor_tcp_packet_received.is_fin_known\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 22usize,
        name: Str::new("acceptor_tcp_packet_received.sojourn_time\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 23usize,
        name: Str::new("acceptor_tcp_tls_started\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 24usize,
        name: Str::new("acceptor_tcp_tls_started.sojourn_time\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 25usize,
        name: Str::new("acceptor_tcp_tls_stream_enqueued\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 26usize,
        name: Str::new("acceptor_tcp_tls_stream_enqueued.sojourn_time\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 27usize,
        name: Str::new("acceptor_tcp_tls_stream_rejected\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 28usize,
        name: Str::new("acceptor_tcp_tls_stream_rejected.sojourn_time\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 29usize,
        name: Str::new("acceptor_tcp_packet_dropped\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 30usize,
        name: Str::new("acceptor_tcp_packet_dropped.reason\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 31usize,
        name: Str::new("acceptor_tcp_packet_dropped.sojourn_time\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 32usize,
        name: Str::new("acceptor_tcp_stream_enqueued\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 33usize,
        name: Str::new("acceptor_tcp_stream_enqueued.sojourn_time\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 34usize,
        name: Str::new("acceptor_tcp_stream_enqueued.blocked_count\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 35usize,
        name: Str::new("acceptor_tcp_io_error\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 36usize,
        name: Str::new("acceptor_tcp_io_error.source\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 37usize,
        name: Str::new("acceptor_tcp_socket_sent\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 38usize,
        name: Str::new("acceptor_tcp_socket_sent.sojourn_time\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 39usize,
        name: Str::new("acceptor_tcp_socket_sent.blocked_count_host\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 40usize,
        name: Str::new("acceptor_tcp_socket_sent.blocked_count_stream\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 41usize,
        name: Str::new("acceptor_tcp_socket_sent.len\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 42usize,
        name: Str::new("acceptor_tcp_socket_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 43usize,
        name: Str::new("acceptor_tcp_socket_received.transfer_time\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 44usize,
        name: Str::new("acceptor_tcp_socket_received.len\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 45usize,
        name: Str::new("acceptor_udp_started\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 46usize,
        name: Str::new("acceptor_udp_datagram_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 47usize,
        name: Str::new("acceptor_udp_datagram_received.len\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 48usize,
        name: Str::new("acceptor_udp_packet_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 49usize,
        name: Str::new("acceptor_udp_packet_received.payload_len\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 50usize,
        name: Str::new("acceptor_udp_packet_received.is_zero_offset\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 51usize,
        name: Str::new("acceptor_udp_packet_received.is_retransmission\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 52usize,
        name: Str::new("acceptor_udp_packet_received.is_fin\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 53usize,
        name: Str::new("acceptor_udp_packet_received.is_fin_known\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 54usize,
        name: Str::new("acceptor_udp_packet_dropped\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 55usize,
        name: Str::new("acceptor_udp_packet_dropped.reason\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 56usize,
        name: Str::new("acceptor_udp_stream_enqueued\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 57usize,
        name: Str::new("acceptor_udp_io_error\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 58usize,
        name: Str::new("acceptor_stream_pruned\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 59usize,
        name: Str::new("acceptor_stream_pruned.sojourn_time\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 60usize,
        name: Str::new("acceptor_stream_pruned.reason\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 61usize,
        name: Str::new("acceptor_stream_dequeued\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 62usize,
        name: Str::new("acceptor_stream_dequeued.sojourn_time\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 63usize,
        name: Str::new("acceptor_stream_dequeued.queue_sojourn_time\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 64usize,
        name: Str::new("stream_write_flushed\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 65usize,
        name: Str::new("stream_write_flushed.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 66usize,
        name: Str::new("stream_write_flushed.conn\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 67usize,
        name: Str::new("stream_write_flushed.provided\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 68usize,
        name: Str::new("stream_write_flushed.committed.total\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 69usize,
        name: Str::new("stream_write_flushed.committed\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 70usize,
        name: Str::new("stream_write_flushed.committed.conn\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 71usize,
        name: Str::new("stream_write_flushed.processing_duration\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 72usize,
        name: Str::new("stream_write_flushed.processing_duration.conn\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 73usize,
        name: Str::new("stream_write_fin_flushed\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 74usize,
        name: Str::new("stream_write_fin_flushed.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 75usize,
        name: Str::new("stream_write_fin_flushed.conn\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 76usize,
        name: Str::new("stream_write_fin_flushed.provided\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 77usize,
        name: Str::new("stream_write_fin_flushed.committed.total\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 78usize,
        name: Str::new("stream_write_fin_flushed.committed\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 79usize,
        name: Str::new("stream_write_fin_flushed.committed.conn\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 80usize,
        name: Str::new("stream_write_fin_flushed.processing_duration\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 81usize,
        name: Str::new("stream_write_fin_flushed.processing_duration.conn\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 82usize,
        name: Str::new("stream_write_blocked\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 83usize,
        name: Str::new("stream_write_blocked.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 84usize,
        name: Str::new("stream_write_blocked.conn\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 85usize,
        name: Str::new("stream_write_blocked.provided\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 86usize,
        name: Str::new("stream_write_blocked.processing_duration\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 87usize,
        name: Str::new("stream_write_blocked.processing_duration.conn\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 88usize,
        name: Str::new("stream_write_errored\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 89usize,
        name: Str::new("stream_write_errored.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 90usize,
        name: Str::new("stream_write_errored.provided\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 91usize,
        name: Str::new("stream_write_errored.processing_duration\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 92usize,
        name: Str::new("stream_write_errored.processing_duration.conn\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 93usize,
        name: Str::new("stream_write_key_updated\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 94usize,
        name: Str::new("stream_write_allocated\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 95usize,
        name: Str::new("stream_write_allocated.conn\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 96usize,
        name: Str::new("stream_write_allocated.allocated_len\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 97usize,
        name: Str::new("stream_write_allocated.allocated_len.conn\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 98usize,
        name: Str::new("stream_write_shutdown\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 99usize,
        name: Str::new("stream_write_shutdown.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 100usize,
        name: Str::new("stream_write_shutdown.buffer_len\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 101usize,
        name: Str::new("stream_write_shutdown.background\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 102usize,
        name: Str::new("stream_write_socket_flushed\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 103usize,
        name: Str::new("stream_write_socket_flushed.conn\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 104usize,
        name: Str::new("stream_write_socket_flushed.provided\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 105usize,
        name: Str::new("stream_write_socket_flushed.committed.total\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 106usize,
        name: Str::new("stream_write_socket_flushed.committed\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 107usize,
        name: Str::new("stream_write_socket_flushed.committed.conn\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 108usize,
        name: Str::new("stream_write_socket_blocked\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 109usize,
        name: Str::new("stream_write_socket_blocked.conn\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 110usize,
        name: Str::new("stream_write_socket_blocked.provided\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 111usize,
        name: Str::new("stream_write_socket_errored\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 112usize,
        name: Str::new("stream_write_socket_errored.provided\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 113usize,
        name: Str::new("stream_read_flushed\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 114usize,
        name: Str::new("stream_read_flushed.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 115usize,
        name: Str::new("stream_read_flushed.conn\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 116usize,
        name: Str::new("stream_read_flushed.capacity\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 117usize,
        name: Str::new("stream_read_flushed.committed.total\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 118usize,
        name: Str::new("stream_read_flushed.committed\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 119usize,
        name: Str::new("stream_read_flushed.committed.conn\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 120usize,
        name: Str::new("stream_read_flushed.processing_duration\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 121usize,
        name: Str::new("stream_read_flushed.processing_duration.conn\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 122usize,
        name: Str::new("stream_read_fin_flushed\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 123usize,
        name: Str::new("stream_read_fin_flushed.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 124usize,
        name: Str::new("stream_read_fin_flushed.conn\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 125usize,
        name: Str::new("stream_read_fin_flushed.capacity\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 126usize,
        name: Str::new("stream_read_fin_flushed.processing_duration\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 127usize,
        name: Str::new("stream_read_fin_flushed.processing_duration.conn\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 128usize,
        name: Str::new("stream_read_blocked\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 129usize,
        name: Str::new("stream_read_blocked.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 130usize,
        name: Str::new("stream_read_blocked.capacity\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 131usize,
        name: Str::new("stream_read_blocked.processing_duration\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 132usize,
        name: Str::new("stream_read_blocked.processing_duration.conn\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 133usize,
        name: Str::new("stream_read_errored\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 134usize,
        name: Str::new("stream_read_errored.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 135usize,
        name: Str::new("stream_read_errored.capacity\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 136usize,
        name: Str::new("stream_read_errored.processing_duration\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 137usize,
        name: Str::new("stream_read_errored.processing_duration.conn\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 138usize,
        name: Str::new("stream_read_key_updated\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 139usize,
        name: Str::new("stream_read_shutdown\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 140usize,
        name: Str::new("stream_read_shutdown.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 141usize,
        name: Str::new("stream_read_shutdown.background\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 142usize,
        name: Str::new("stream_read_socket_flushed\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 143usize,
        name: Str::new("stream_read_socket_flushed.conn\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 144usize,
        name: Str::new("stream_read_socket_flushed.capacity\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 145usize,
        name: Str::new("stream_read_socket_flushed.committed.total\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 146usize,
        name: Str::new("stream_read_socket_flushed.committed\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 147usize,
        name: Str::new("stream_read_socket_flushed.committed.conn\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 148usize,
        name: Str::new("stream_read_socket_blocked\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 149usize,
        name: Str::new("stream_read_socket_blocked.conn\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 150usize,
        name: Str::new("stream_read_socket_blocked.capacity\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 151usize,
        name: Str::new("stream_read_socket_errored\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 152usize,
        name: Str::new("stream_read_socket_errored.capacity\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 153usize,
        name: Str::new("stream_decrypt_packet\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 154usize,
        name: Str::new("stream_decrypt_packet.decrypted_in_place\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 155usize,
        name: Str::new("stream_decrypt_packet.forced_copy\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 156usize,
        name: Str::new("stream_decrypt_packet.required_application_buffer\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 157usize,
        name: Str::new("stream_tcp_connect\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 158usize,
        name: Str::new("stream_tcp_connect.error\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 159usize,
        name: Str::new("stream_tcp_connect.tcp_latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 160usize,
        name: Str::new("stream_connect\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 161usize,
        name: Str::new("stream_connect.error\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 162usize,
        name: Str::new("stream_connect.tcp\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 163usize,
        name: Str::new("stream_connect.handshake\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 164usize,
        name: Str::new("stream_connect_error\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 165usize,
        name: Str::new("stream_connect_error.reason\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 166usize,
        name: Str::new("stream_connect_error.latency\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 167usize,
        name: Str::new("stream_packet_transmitted\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 168usize,
        name: Str::new("stream_packet_transmitted.packet_len\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 169usize,
        name: Str::new("stream_packet_transmitted.payload_len.total\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 170usize,
        name: Str::new("stream_packet_transmitted.payload_len\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 171usize,
        name: Str::new("stream_packet_transmitted.payload_len.conn\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 172usize,
        name: Str::new("stream_packet_transmitted.retransmission\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 173usize,
        name: Str::new("stream_probe_transmitted\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 174usize,
        name: Str::new("stream_probe_transmitted.packet_len\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 175usize,
        name: Str::new("stream_packet_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 176usize,
        name: Str::new("stream_packet_received.packet_len\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 177usize,
        name: Str::new("stream_packet_received.payload_len.total\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 178usize,
        name: Str::new("stream_packet_received.payload_len\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 179usize,
        name: Str::new("stream_packet_received.payload_len.conn\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 180usize,
        name: Str::new("stream_packet_received.retransmission\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 181usize,
        name: Str::new("stream_packet_lost\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 182usize,
        name: Str::new("stream_packet_lost.packet_len\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 183usize,
        name: Str::new("stream_packet_lost.payload_len.total\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 184usize,
        name: Str::new("stream_packet_lost.payload_len\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 185usize,
        name: Str::new("stream_packet_lost.payload_len.conn\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 186usize,
        name: Str::new("stream_packet_lost.lifetime\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 187usize,
        name: Str::new("stream_packet_lost.retransmission\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 188usize,
        name: Str::new("stream_packet_acked\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 189usize,
        name: Str::new("stream_packet_acked.packet_len\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 190usize,
        name: Str::new("stream_packet_acked.payload_len.total\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 191usize,
        name: Str::new("stream_packet_acked.payload_len\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 192usize,
        name: Str::new("stream_packet_acked.payload_len.conn\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 193usize,
        name: Str::new("stream_packet_acked.lifetime\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 194usize,
        name: Str::new("stream_packet_acked.retransmission\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 195usize,
        name: Str::new("stream_packet_spuriously_retransmitted\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 196usize,
        name: Str::new("stream_packet_spuriously_retransmitted.packet_len\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 197usize,
        name: Str::new("stream_packet_spuriously_retransmitted.payload_len.total\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 198usize,
        name: Str::new("stream_packet_spuriously_retransmitted.payload_len\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 199usize,
        name: Str::new("stream_packet_spuriously_retransmitted.payload_len.conn\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 200usize,
        name: Str::new("stream_packet_spuriously_retransmitted.retransmission\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 201usize,
        name: Str::new("stream_max_data_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 202usize,
        name: Str::new("stream_max_data_received.increase.total\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 203usize,
        name: Str::new("stream_max_data_received.increase\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 204usize,
        name: Str::new("stream_control_packet_transmitted\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 205usize,
        name: Str::new("stream_control_packet_transmitted.packet_len\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 206usize,
        name: Str::new("stream_control_packet_transmitted.control_data_len\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 207usize,
        name: Str::new("stream_control_packet_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 208usize,
        name: Str::new("stream_control_packet_received.packet_len\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 209usize,
        name: Str::new("stream_control_packet_received.control_data_len\0"),
        units: Units::Bytes,
    }
    .build(),
    info::Builder {
        id: 210usize,
        name: Str::new("stream_control_packet_received.authenticated\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 211usize,
        name: Str::new("stream_receiver_errored\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 212usize,
        name: Str::new("stream_sender_errored\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 213usize,
        name: Str::new("connection_closed\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 214usize,
        name: Str::new("endpoint_initialized\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 215usize,
        name: Str::new("endpoint_initialized.acceptor.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 216usize,
        name: Str::new("endpoint_initialized.handshake.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 217usize,
        name: Str::new("endpoint_initialized.tcp\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 218usize,
        name: Str::new("endpoint_initialized.udp\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 219usize,
        name: Str::new("dc_connection_timeout\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 220usize,
        name: Str::new("dc_connection_timeout.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 221usize,
        name: Str::new("path_secret_map_initialized\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 222usize,
        name: Str::new("path_secret_map_initialized.capacity\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 223usize,
        name: Str::new("path_secret_map_uninitialized\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 224usize,
        name: Str::new("path_secret_map_uninitialized.capacity\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 225usize,
        name: Str::new("path_secret_map_uninitialized.entries\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 226usize,
        name: Str::new("path_secret_map_uninitialized.lifetime\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 227usize,
        name: Str::new("path_secret_map_background_handshake_requested\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 228usize,
        name: Str::new("path_secret_map_background_handshake_requested.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 229usize,
        name: Str::new("path_secret_map_entry_inserted\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 230usize,
        name: Str::new("path_secret_map_entry_inserted.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 231usize,
        name: Str::new("path_secret_map_entry_ready\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 232usize,
        name: Str::new("path_secret_map_entry_ready.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 233usize,
        name: Str::new("path_secret_map_entry_replaced\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 234usize,
        name: Str::new("path_secret_map_entry_replaced.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 235usize,
        name: Str::new("path_secret_map_id_entry_evicted\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 236usize,
        name: Str::new("path_secret_map_id_entry_evicted.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 237usize,
        name: Str::new("path_secret_map_id_entry_evicted.age\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 238usize,
        name: Str::new("path_secret_map_address_entry_evicted\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 239usize,
        name: Str::new("path_secret_map_address_entry_evicted.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 240usize,
        name: Str::new("path_secret_map_address_entry_evicted.age\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 241usize,
        name: Str::new("unknown_path_secret_packet_sent\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 242usize,
        name: Str::new("unknown_path_secret_packet_sent.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 243usize,
        name: Str::new("unknown_path_secret_packet_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 244usize,
        name: Str::new("unknown_path_secret_packet_received.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 245usize,
        name: Str::new("unknown_path_secret_packet_accepted\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 246usize,
        name: Str::new("unknown_path_secret_packet_accepted.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 247usize,
        name: Str::new("unknown_path_secret_packet_rejected\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 248usize,
        name: Str::new("unknown_path_secret_packet_rejected.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 249usize,
        name: Str::new("unknown_path_secret_packet_dropped\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 250usize,
        name: Str::new("unknown_path_secret_packet_dropped.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 251usize,
        name: Str::new("key_accepted\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 252usize,
        name: Str::new("key_accepted.gap\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 253usize,
        name: Str::new("key_accepted.forward_shift\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 254usize,
        name: Str::new("replay_definitely_detected\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 255usize,
        name: Str::new("replay_potentially_detected\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 256usize,
        name: Str::new("replay_potentially_detected.gap\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 257usize,
        name: Str::new("replay_detected_packet_sent\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 258usize,
        name: Str::new("replay_detected_packet_sent.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 259usize,
        name: Str::new("replay_detected_packet_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 260usize,
        name: Str::new("replay_detected_packet_received.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 261usize,
        name: Str::new("replay_detected_packet_accepted\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 262usize,
        name: Str::new("replay_detected_packet_accepted.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 263usize,
        name: Str::new("replay_detected_packet_rejected\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 264usize,
        name: Str::new("replay_detected_packet_rejected.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 265usize,
        name: Str::new("replay_detected_packet_dropped\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 266usize,
        name: Str::new("replay_detected_packet_dropped.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 267usize,
        name: Str::new("stale_key_packet_sent\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 268usize,
        name: Str::new("stale_key_packet_sent.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 269usize,
        name: Str::new("stale_key_packet_received\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 270usize,
        name: Str::new("stale_key_packet_received.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 271usize,
        name: Str::new("stale_key_packet_accepted\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 272usize,
        name: Str::new("stale_key_packet_accepted.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 273usize,
        name: Str::new("stale_key_packet_rejected\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 274usize,
        name: Str::new("stale_key_packet_rejected.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 275usize,
        name: Str::new("stale_key_packet_dropped\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 276usize,
        name: Str::new("stale_key_packet_dropped.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 277usize,
        name: Str::new("path_secret_map_address_cache_accessed\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 278usize,
        name: Str::new("path_secret_map_address_cache_accessed.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 279usize,
        name: Str::new("path_secret_map_address_cache_accessed.hit\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 280usize,
        name: Str::new("path_secret_map_address_cache_accessed_hit\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 281usize,
        name: Str::new("path_secret_map_address_cache_accessed_hit.peer_address.protocol\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 282usize,
        name: Str::new("path_secret_map_address_cache_accessed_hit.age\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 283usize,
        name: Str::new("path_secret_map_id_cache_accessed\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 284usize,
        name: Str::new("path_secret_map_id_cache_accessed.hit\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 285usize,
        name: Str::new("path_secret_map_id_cache_accessed_hit\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 286usize,
        name: Str::new("path_secret_map_id_cache_accessed_hit.age\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 287usize,
        name: Str::new("path_secret_map_cleaner_cycled\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 288usize,
        name: Str::new("path_secret_map_cleaner_cycled.entries.id\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 289usize,
        name: Str::new("path_secret_map_cleaner_cycled.entries.id.retired\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 290usize,
        name: Str::new("path_secret_map_cleaner_cycled.entries.id.active\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 291usize,
        name: Str::new("path_secret_map_cleaner_cycled.entries.id.active.utilization\0"),
        units: Units::Percent,
    }
    .build(),
    info::Builder {
        id: 292usize,
        name: Str::new("path_secret_map_cleaner_cycled.entries.id.utilization\0"),
        units: Units::Percent,
    }
    .build(),
    info::Builder {
        id: 293usize,
        name: Str::new("path_secret_map_cleaner_cycled.entries.id.utilization.initial\0"),
        units: Units::Percent,
    }
    .build(),
    info::Builder {
        id: 294usize,
        name: Str::new("path_secret_map_cleaner_cycled.entries.address\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 295usize,
        name: Str::new("path_secret_map_cleaner_cycled.entries.address.active\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 296usize,
        name: Str::new("path_secret_map_cleaner_cycled.entries.address.active.utilization\0"),
        units: Units::Percent,
    }
    .build(),
    info::Builder {
        id: 297usize,
        name: Str::new("path_secret_map_cleaner_cycled.entries.address.retired\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 298usize,
        name: Str::new("path_secret_map_cleaner_cycled.entries.address.utilization\0"),
        units: Units::Percent,
    }
    .build(),
    info::Builder {
        id: 299usize,
        name: Str::new("path_secret_map_cleaner_cycled.entries.address.utilization.initial\0"),
        units: Units::Percent,
    }
    .build(),
    info::Builder {
        id: 300usize,
        name: Str::new("path_secret_map_cleaner_cycled.handshake_requests\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 301usize,
        name: Str::new("path_secret_map_cleaner_cycled.handshake_requests.skipped\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 302usize,
        name: Str::new("path_secret_map_cleaner_cycled.handshake_lock_duration\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 303usize,
        name: Str::new("path_secret_map_cleaner_cycled.total_duration\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 304usize,
        name: Str::new("path_secret_map_id_write_lock\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 305usize,
        name: Str::new("path_secret_map_id_write_lock.acquire\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 306usize,
        name: Str::new("path_secret_map_id_write_lock.duration\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 307usize,
        name: Str::new("path_secret_map_address_write_lock\0"),
        units: Units::None,
    }
    .build(),
    info::Builder {
        id: 308usize,
        name: Str::new("path_secret_map_address_write_lock.acquire\0"),
        units: Units::Duration,
    }
    .build(),
    info::Builder {
        id: 309usize,
        name: Str::new("path_secret_map_address_write_lock.duration\0"),
        units: Units::Duration,
    }
    .build(),
];
#[derive(Debug)]
#[allow(dead_code)]
pub struct ConnectionContext {
    start_time: crate::event::Timestamp,
    ctr_16: AtomicU64,
    ctr_19: AtomicU64,
    ctr_21: AtomicU64,
    ctr_22: AtomicU64,
    ctr_25: AtomicU64,
    ctr_27: AtomicU64,
    ctr_28: AtomicU64,
    ctr_31: AtomicU64,
    ctr_34: AtomicU64,
    ctr_35: AtomicU64,
    ctr_37: AtomicU64,
    ctr_39: AtomicU64,
    ctr_42: AtomicU64,
    ctr_43: AtomicU64,
    ctr_46: AtomicU64,
    ctr_49: AtomicU64,
    ctr_51: AtomicU64,
    ctr_52: AtomicU64,
    ctr_55: AtomicU64,
    ctr_58: AtomicU64,
    ctr_61: AtomicU64,
    ctr_62: AtomicU64,
    ctr_65: AtomicU64,
    ctr_66: AtomicU64,
    ctr_73: AtomicU64,
    ctr_77: AtomicU64,
    ctr_80: AtomicU64,
    ctr_84: AtomicU64,
    ctr_88: AtomicU64,
}
pub struct Subscriber<R: Registry> {
    #[allow(dead_code)]
    counters: Box<[R::Counter; 105usize]>,
    #[allow(dead_code)]
    bool_counters: Box<[R::BoolCounter; 21usize]>,
    #[allow(dead_code)]
    nominal_counters: Box<[R::NominalCounter]>,
    #[allow(dead_code)]
    nominal_counter_offsets: Box<[usize; 34usize]>,
    #[allow(dead_code)]
    measures: Box<[R::Measure; 125usize]>,
    #[allow(dead_code)]
    gauges: Box<[R::Gauge; 0usize]>,
    #[allow(dead_code)]
    timers: Box<[R::Timer; 25usize]>,
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
        let mut counters = Vec::with_capacity(105usize);
        let mut bool_counters = Vec::with_capacity(21usize);
        let mut nominal_counters = Vec::with_capacity(34usize);
        let mut nominal_counter_offsets = Vec::with_capacity(34usize);
        let mut measures = Vec::with_capacity(125usize);
        let mut gauges = Vec::with_capacity(0usize);
        let mut timers = Vec::with_capacity(25usize);
        let mut nominal_timers = Vec::with_capacity(0usize);
        let mut nominal_timer_offsets = Vec::with_capacity(0usize);
        counters.push(registry.register_counter(&INFO[0usize]));
        counters.push(registry.register_counter(&INFO[2usize]));
        counters.push(registry.register_counter(&INFO[8usize]));
        counters.push(registry.register_counter(&INFO[9usize]));
        counters.push(registry.register_counter(&INFO[13usize]));
        counters.push(registry.register_counter(&INFO[15usize]));
        counters.push(registry.register_counter(&INFO[18usize]));
        counters.push(registry.register_counter(&INFO[23usize]));
        counters.push(registry.register_counter(&INFO[25usize]));
        counters.push(registry.register_counter(&INFO[27usize]));
        counters.push(registry.register_counter(&INFO[29usize]));
        counters.push(registry.register_counter(&INFO[32usize]));
        counters.push(registry.register_counter(&INFO[35usize]));
        counters.push(registry.register_counter(&INFO[37usize]));
        counters.push(registry.register_counter(&INFO[39usize]));
        counters.push(registry.register_counter(&INFO[42usize]));
        counters.push(registry.register_counter(&INFO[45usize]));
        counters.push(registry.register_counter(&INFO[46usize]));
        counters.push(registry.register_counter(&INFO[48usize]));
        counters.push(registry.register_counter(&INFO[54usize]));
        counters.push(registry.register_counter(&INFO[56usize]));
        counters.push(registry.register_counter(&INFO[57usize]));
        counters.push(registry.register_counter(&INFO[58usize]));
        counters.push(registry.register_counter(&INFO[61usize]));
        counters.push(registry.register_counter(&INFO[64usize]));
        counters.push(registry.register_counter(&INFO[68usize]));
        counters.push(registry.register_counter(&INFO[73usize]));
        counters.push(registry.register_counter(&INFO[77usize]));
        counters.push(registry.register_counter(&INFO[82usize]));
        counters.push(registry.register_counter(&INFO[88usize]));
        counters.push(registry.register_counter(&INFO[93usize]));
        counters.push(registry.register_counter(&INFO[94usize]));
        counters.push(registry.register_counter(&INFO[98usize]));
        counters.push(registry.register_counter(&INFO[102usize]));
        counters.push(registry.register_counter(&INFO[105usize]));
        counters.push(registry.register_counter(&INFO[108usize]));
        counters.push(registry.register_counter(&INFO[111usize]));
        counters.push(registry.register_counter(&INFO[113usize]));
        counters.push(registry.register_counter(&INFO[117usize]));
        counters.push(registry.register_counter(&INFO[122usize]));
        counters.push(registry.register_counter(&INFO[128usize]));
        counters.push(registry.register_counter(&INFO[133usize]));
        counters.push(registry.register_counter(&INFO[138usize]));
        counters.push(registry.register_counter(&INFO[139usize]));
        counters.push(registry.register_counter(&INFO[142usize]));
        counters.push(registry.register_counter(&INFO[145usize]));
        counters.push(registry.register_counter(&INFO[148usize]));
        counters.push(registry.register_counter(&INFO[151usize]));
        counters.push(registry.register_counter(&INFO[153usize]));
        counters.push(registry.register_counter(&INFO[157usize]));
        counters.push(registry.register_counter(&INFO[160usize]));
        counters.push(registry.register_counter(&INFO[164usize]));
        counters.push(registry.register_counter(&INFO[167usize]));
        counters.push(registry.register_counter(&INFO[169usize]));
        counters.push(registry.register_counter(&INFO[173usize]));
        counters.push(registry.register_counter(&INFO[175usize]));
        counters.push(registry.register_counter(&INFO[177usize]));
        counters.push(registry.register_counter(&INFO[181usize]));
        counters.push(registry.register_counter(&INFO[183usize]));
        counters.push(registry.register_counter(&INFO[188usize]));
        counters.push(registry.register_counter(&INFO[190usize]));
        counters.push(registry.register_counter(&INFO[195usize]));
        counters.push(registry.register_counter(&INFO[197usize]));
        counters.push(registry.register_counter(&INFO[201usize]));
        counters.push(registry.register_counter(&INFO[202usize]));
        counters.push(registry.register_counter(&INFO[204usize]));
        counters.push(registry.register_counter(&INFO[207usize]));
        counters.push(registry.register_counter(&INFO[211usize]));
        counters.push(registry.register_counter(&INFO[212usize]));
        counters.push(registry.register_counter(&INFO[213usize]));
        counters.push(registry.register_counter(&INFO[214usize]));
        counters.push(registry.register_counter(&INFO[219usize]));
        counters.push(registry.register_counter(&INFO[221usize]));
        counters.push(registry.register_counter(&INFO[223usize]));
        counters.push(registry.register_counter(&INFO[227usize]));
        counters.push(registry.register_counter(&INFO[229usize]));
        counters.push(registry.register_counter(&INFO[231usize]));
        counters.push(registry.register_counter(&INFO[233usize]));
        counters.push(registry.register_counter(&INFO[235usize]));
        counters.push(registry.register_counter(&INFO[238usize]));
        counters.push(registry.register_counter(&INFO[241usize]));
        counters.push(registry.register_counter(&INFO[243usize]));
        counters.push(registry.register_counter(&INFO[245usize]));
        counters.push(registry.register_counter(&INFO[247usize]));
        counters.push(registry.register_counter(&INFO[249usize]));
        counters.push(registry.register_counter(&INFO[251usize]));
        counters.push(registry.register_counter(&INFO[254usize]));
        counters.push(registry.register_counter(&INFO[255usize]));
        counters.push(registry.register_counter(&INFO[257usize]));
        counters.push(registry.register_counter(&INFO[259usize]));
        counters.push(registry.register_counter(&INFO[261usize]));
        counters.push(registry.register_counter(&INFO[263usize]));
        counters.push(registry.register_counter(&INFO[265usize]));
        counters.push(registry.register_counter(&INFO[267usize]));
        counters.push(registry.register_counter(&INFO[269usize]));
        counters.push(registry.register_counter(&INFO[271usize]));
        counters.push(registry.register_counter(&INFO[273usize]));
        counters.push(registry.register_counter(&INFO[275usize]));
        counters.push(registry.register_counter(&INFO[277usize]));
        counters.push(registry.register_counter(&INFO[280usize]));
        counters.push(registry.register_counter(&INFO[283usize]));
        counters.push(registry.register_counter(&INFO[285usize]));
        counters.push(registry.register_counter(&INFO[287usize]));
        counters.push(registry.register_counter(&INFO[304usize]));
        counters.push(registry.register_counter(&INFO[307usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[20usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[21usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[50usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[51usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[52usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[53usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[101usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[141usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[154usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[158usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[161usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[172usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[180usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[187usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[194usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[200usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[210usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[217usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[218usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[279usize]));
        bool_counters.push(registry.register_bool_counter(&INFO[284usize]));
        {
            #[allow(unused_imports)]
            use api::*;
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <AcceptorTcpStreamDropReason as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[14usize], variant));
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
                        .push(registry.register_nominal_counter(&INFO[30usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
            {
                let offset = nominal_counters.len();
                let mut count = 0;
                for variant in <AcceptorTcpIoErrorSource as AsVariant>::VARIANTS.iter() {
                    nominal_counters
                        .push(registry.register_nominal_counter(&INFO[36usize], variant));
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
                        .push(registry.register_nominal_counter(&INFO[55usize], variant));
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
                        .push(registry.register_nominal_counter(&INFO[60usize], variant));
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
                        .push(registry.register_nominal_counter(&INFO[162usize], variant));
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
                        .push(registry.register_nominal_counter(&INFO[163usize], variant));
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
                        .push(registry.register_nominal_counter(&INFO[215usize], variant));
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
                        .push(registry.register_nominal_counter(&INFO[216usize], variant));
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
                        .push(registry.register_nominal_counter(&INFO[220usize], variant));
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
                        .push(registry.register_nominal_counter(&INFO[228usize], variant));
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
                        .push(registry.register_nominal_counter(&INFO[230usize], variant));
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
                        .push(registry.register_nominal_counter(&INFO[232usize], variant));
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
                        .push(registry.register_nominal_counter(&INFO[234usize], variant));
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
                        .push(registry.register_nominal_counter(&INFO[236usize], variant));
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
                        .push(registry.register_nominal_counter(&INFO[239usize], variant));
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
                        .push(registry.register_nominal_counter(&INFO[242usize], variant));
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
                        .push(registry.register_nominal_counter(&INFO[244usize], variant));
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
                        .push(registry.register_nominal_counter(&INFO[246usize], variant));
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
                        .push(registry.register_nominal_counter(&INFO[248usize], variant));
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
                        .push(registry.register_nominal_counter(&INFO[250usize], variant));
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
                        .push(registry.register_nominal_counter(&INFO[258usize], variant));
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
                        .push(registry.register_nominal_counter(&INFO[260usize], variant));
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
                        .push(registry.register_nominal_counter(&INFO[262usize], variant));
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
                        .push(registry.register_nominal_counter(&INFO[264usize], variant));
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
                        .push(registry.register_nominal_counter(&INFO[266usize], variant));
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
                        .push(registry.register_nominal_counter(&INFO[268usize], variant));
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
                        .push(registry.register_nominal_counter(&INFO[270usize], variant));
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
                        .push(registry.register_nominal_counter(&INFO[272usize], variant));
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
                        .push(registry.register_nominal_counter(&INFO[274usize], variant));
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
                        .push(registry.register_nominal_counter(&INFO[276usize], variant));
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
                        .push(registry.register_nominal_counter(&INFO[278usize], variant));
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
                        .push(registry.register_nominal_counter(&INFO[281usize], variant));
                    count += 1;
                }
                debug_assert_ne!(count, 0, "field type needs at least one variant");
                nominal_counter_offsets.push(offset);
            }
        }
        measures.push(registry.register_measure(&INFO[1usize]));
        measures.push(registry.register_measure(&INFO[3usize]));
        measures.push(registry.register_measure(&INFO[4usize]));
        measures.push(registry.register_measure(&INFO[5usize]));
        measures.push(registry.register_measure(&INFO[7usize]));
        measures.push(registry.register_measure(&INFO[10usize]));
        measures.push(registry.register_measure(&INFO[11usize]));
        measures.push(registry.register_measure(&INFO[12usize]));
        measures.push(registry.register_measure(&INFO[17usize]));
        measures.push(registry.register_measure(&INFO[19usize]));
        measures.push(registry.register_measure(&INFO[34usize]));
        measures.push(registry.register_measure(&INFO[40usize]));
        measures.push(registry.register_measure(&INFO[41usize]));
        measures.push(registry.register_measure(&INFO[44usize]));
        measures.push(registry.register_measure(&INFO[47usize]));
        measures.push(registry.register_measure(&INFO[49usize]));
        measures.push(registry.register_measure(&INFO[66usize]));
        measures.push(registry.register_measure(&INFO[67usize]));
        measures.push(registry.register_measure(&INFO[69usize]));
        measures.push(registry.register_measure(&INFO[70usize]));
        measures.push(registry.register_measure(&INFO[71usize]));
        measures.push(registry.register_measure(&INFO[72usize]));
        measures.push(registry.register_measure(&INFO[75usize]));
        measures.push(registry.register_measure(&INFO[76usize]));
        measures.push(registry.register_measure(&INFO[78usize]));
        measures.push(registry.register_measure(&INFO[79usize]));
        measures.push(registry.register_measure(&INFO[80usize]));
        measures.push(registry.register_measure(&INFO[81usize]));
        measures.push(registry.register_measure(&INFO[84usize]));
        measures.push(registry.register_measure(&INFO[85usize]));
        measures.push(registry.register_measure(&INFO[86usize]));
        measures.push(registry.register_measure(&INFO[87usize]));
        measures.push(registry.register_measure(&INFO[90usize]));
        measures.push(registry.register_measure(&INFO[91usize]));
        measures.push(registry.register_measure(&INFO[92usize]));
        measures.push(registry.register_measure(&INFO[95usize]));
        measures.push(registry.register_measure(&INFO[96usize]));
        measures.push(registry.register_measure(&INFO[97usize]));
        measures.push(registry.register_measure(&INFO[100usize]));
        measures.push(registry.register_measure(&INFO[103usize]));
        measures.push(registry.register_measure(&INFO[104usize]));
        measures.push(registry.register_measure(&INFO[106usize]));
        measures.push(registry.register_measure(&INFO[107usize]));
        measures.push(registry.register_measure(&INFO[109usize]));
        measures.push(registry.register_measure(&INFO[110usize]));
        measures.push(registry.register_measure(&INFO[112usize]));
        measures.push(registry.register_measure(&INFO[115usize]));
        measures.push(registry.register_measure(&INFO[116usize]));
        measures.push(registry.register_measure(&INFO[118usize]));
        measures.push(registry.register_measure(&INFO[119usize]));
        measures.push(registry.register_measure(&INFO[120usize]));
        measures.push(registry.register_measure(&INFO[121usize]));
        measures.push(registry.register_measure(&INFO[124usize]));
        measures.push(registry.register_measure(&INFO[125usize]));
        measures.push(registry.register_measure(&INFO[126usize]));
        measures.push(registry.register_measure(&INFO[127usize]));
        measures.push(registry.register_measure(&INFO[130usize]));
        measures.push(registry.register_measure(&INFO[131usize]));
        measures.push(registry.register_measure(&INFO[132usize]));
        measures.push(registry.register_measure(&INFO[135usize]));
        measures.push(registry.register_measure(&INFO[136usize]));
        measures.push(registry.register_measure(&INFO[137usize]));
        measures.push(registry.register_measure(&INFO[143usize]));
        measures.push(registry.register_measure(&INFO[144usize]));
        measures.push(registry.register_measure(&INFO[146usize]));
        measures.push(registry.register_measure(&INFO[147usize]));
        measures.push(registry.register_measure(&INFO[149usize]));
        measures.push(registry.register_measure(&INFO[150usize]));
        measures.push(registry.register_measure(&INFO[152usize]));
        measures.push(registry.register_measure(&INFO[155usize]));
        measures.push(registry.register_measure(&INFO[156usize]));
        measures.push(registry.register_measure(&INFO[168usize]));
        measures.push(registry.register_measure(&INFO[170usize]));
        measures.push(registry.register_measure(&INFO[171usize]));
        measures.push(registry.register_measure(&INFO[174usize]));
        measures.push(registry.register_measure(&INFO[176usize]));
        measures.push(registry.register_measure(&INFO[178usize]));
        measures.push(registry.register_measure(&INFO[179usize]));
        measures.push(registry.register_measure(&INFO[182usize]));
        measures.push(registry.register_measure(&INFO[184usize]));
        measures.push(registry.register_measure(&INFO[185usize]));
        measures.push(registry.register_measure(&INFO[186usize]));
        measures.push(registry.register_measure(&INFO[189usize]));
        measures.push(registry.register_measure(&INFO[191usize]));
        measures.push(registry.register_measure(&INFO[192usize]));
        measures.push(registry.register_measure(&INFO[193usize]));
        measures.push(registry.register_measure(&INFO[196usize]));
        measures.push(registry.register_measure(&INFO[198usize]));
        measures.push(registry.register_measure(&INFO[199usize]));
        measures.push(registry.register_measure(&INFO[203usize]));
        measures.push(registry.register_measure(&INFO[205usize]));
        measures.push(registry.register_measure(&INFO[206usize]));
        measures.push(registry.register_measure(&INFO[208usize]));
        measures.push(registry.register_measure(&INFO[209usize]));
        measures.push(registry.register_measure(&INFO[222usize]));
        measures.push(registry.register_measure(&INFO[224usize]));
        measures.push(registry.register_measure(&INFO[225usize]));
        measures.push(registry.register_measure(&INFO[226usize]));
        measures.push(registry.register_measure(&INFO[237usize]));
        measures.push(registry.register_measure(&INFO[240usize]));
        measures.push(registry.register_measure(&INFO[252usize]));
        measures.push(registry.register_measure(&INFO[253usize]));
        measures.push(registry.register_measure(&INFO[256usize]));
        measures.push(registry.register_measure(&INFO[282usize]));
        measures.push(registry.register_measure(&INFO[286usize]));
        measures.push(registry.register_measure(&INFO[288usize]));
        measures.push(registry.register_measure(&INFO[289usize]));
        measures.push(registry.register_measure(&INFO[290usize]));
        measures.push(registry.register_measure(&INFO[291usize]));
        measures.push(registry.register_measure(&INFO[292usize]));
        measures.push(registry.register_measure(&INFO[293usize]));
        measures.push(registry.register_measure(&INFO[294usize]));
        measures.push(registry.register_measure(&INFO[295usize]));
        measures.push(registry.register_measure(&INFO[296usize]));
        measures.push(registry.register_measure(&INFO[297usize]));
        measures.push(registry.register_measure(&INFO[298usize]));
        measures.push(registry.register_measure(&INFO[299usize]));
        measures.push(registry.register_measure(&INFO[300usize]));
        measures.push(registry.register_measure(&INFO[301usize]));
        measures.push(registry.register_measure(&INFO[302usize]));
        measures.push(registry.register_measure(&INFO[303usize]));
        measures.push(registry.register_measure(&INFO[305usize]));
        measures.push(registry.register_measure(&INFO[306usize]));
        measures.push(registry.register_measure(&INFO[308usize]));
        measures.push(registry.register_measure(&INFO[309usize]));
        timers.push(registry.register_timer(&INFO[6usize]));
        timers.push(registry.register_timer(&INFO[16usize]));
        timers.push(registry.register_timer(&INFO[22usize]));
        timers.push(registry.register_timer(&INFO[24usize]));
        timers.push(registry.register_timer(&INFO[26usize]));
        timers.push(registry.register_timer(&INFO[28usize]));
        timers.push(registry.register_timer(&INFO[31usize]));
        timers.push(registry.register_timer(&INFO[33usize]));
        timers.push(registry.register_timer(&INFO[38usize]));
        timers.push(registry.register_timer(&INFO[43usize]));
        timers.push(registry.register_timer(&INFO[59usize]));
        timers.push(registry.register_timer(&INFO[62usize]));
        timers.push(registry.register_timer(&INFO[63usize]));
        timers.push(registry.register_timer(&INFO[65usize]));
        timers.push(registry.register_timer(&INFO[74usize]));
        timers.push(registry.register_timer(&INFO[83usize]));
        timers.push(registry.register_timer(&INFO[89usize]));
        timers.push(registry.register_timer(&INFO[99usize]));
        timers.push(registry.register_timer(&INFO[114usize]));
        timers.push(registry.register_timer(&INFO[123usize]));
        timers.push(registry.register_timer(&INFO[129usize]));
        timers.push(registry.register_timer(&INFO[134usize]));
        timers.push(registry.register_timer(&INFO[140usize]));
        timers.push(registry.register_timer(&INFO[159usize]));
        timers.push(registry.register_timer(&INFO[166usize]));
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
                1usize => (&INFO[2usize], entry),
                2usize => (&INFO[8usize], entry),
                3usize => (&INFO[9usize], entry),
                4usize => (&INFO[13usize], entry),
                5usize => (&INFO[15usize], entry),
                6usize => (&INFO[18usize], entry),
                7usize => (&INFO[23usize], entry),
                8usize => (&INFO[25usize], entry),
                9usize => (&INFO[27usize], entry),
                10usize => (&INFO[29usize], entry),
                11usize => (&INFO[32usize], entry),
                12usize => (&INFO[35usize], entry),
                13usize => (&INFO[37usize], entry),
                14usize => (&INFO[39usize], entry),
                15usize => (&INFO[42usize], entry),
                16usize => (&INFO[45usize], entry),
                17usize => (&INFO[46usize], entry),
                18usize => (&INFO[48usize], entry),
                19usize => (&INFO[54usize], entry),
                20usize => (&INFO[56usize], entry),
                21usize => (&INFO[57usize], entry),
                22usize => (&INFO[58usize], entry),
                23usize => (&INFO[61usize], entry),
                24usize => (&INFO[64usize], entry),
                25usize => (&INFO[68usize], entry),
                26usize => (&INFO[73usize], entry),
                27usize => (&INFO[77usize], entry),
                28usize => (&INFO[82usize], entry),
                29usize => (&INFO[88usize], entry),
                30usize => (&INFO[93usize], entry),
                31usize => (&INFO[94usize], entry),
                32usize => (&INFO[98usize], entry),
                33usize => (&INFO[102usize], entry),
                34usize => (&INFO[105usize], entry),
                35usize => (&INFO[108usize], entry),
                36usize => (&INFO[111usize], entry),
                37usize => (&INFO[113usize], entry),
                38usize => (&INFO[117usize], entry),
                39usize => (&INFO[122usize], entry),
                40usize => (&INFO[128usize], entry),
                41usize => (&INFO[133usize], entry),
                42usize => (&INFO[138usize], entry),
                43usize => (&INFO[139usize], entry),
                44usize => (&INFO[142usize], entry),
                45usize => (&INFO[145usize], entry),
                46usize => (&INFO[148usize], entry),
                47usize => (&INFO[151usize], entry),
                48usize => (&INFO[153usize], entry),
                49usize => (&INFO[157usize], entry),
                50usize => (&INFO[160usize], entry),
                51usize => (&INFO[164usize], entry),
                52usize => (&INFO[167usize], entry),
                53usize => (&INFO[169usize], entry),
                54usize => (&INFO[173usize], entry),
                55usize => (&INFO[175usize], entry),
                56usize => (&INFO[177usize], entry),
                57usize => (&INFO[181usize], entry),
                58usize => (&INFO[183usize], entry),
                59usize => (&INFO[188usize], entry),
                60usize => (&INFO[190usize], entry),
                61usize => (&INFO[195usize], entry),
                62usize => (&INFO[197usize], entry),
                63usize => (&INFO[201usize], entry),
                64usize => (&INFO[202usize], entry),
                65usize => (&INFO[204usize], entry),
                66usize => (&INFO[207usize], entry),
                67usize => (&INFO[211usize], entry),
                68usize => (&INFO[212usize], entry),
                69usize => (&INFO[213usize], entry),
                70usize => (&INFO[214usize], entry),
                71usize => (&INFO[219usize], entry),
                72usize => (&INFO[221usize], entry),
                73usize => (&INFO[223usize], entry),
                74usize => (&INFO[227usize], entry),
                75usize => (&INFO[229usize], entry),
                76usize => (&INFO[231usize], entry),
                77usize => (&INFO[233usize], entry),
                78usize => (&INFO[235usize], entry),
                79usize => (&INFO[238usize], entry),
                80usize => (&INFO[241usize], entry),
                81usize => (&INFO[243usize], entry),
                82usize => (&INFO[245usize], entry),
                83usize => (&INFO[247usize], entry),
                84usize => (&INFO[249usize], entry),
                85usize => (&INFO[251usize], entry),
                86usize => (&INFO[254usize], entry),
                87usize => (&INFO[255usize], entry),
                88usize => (&INFO[257usize], entry),
                89usize => (&INFO[259usize], entry),
                90usize => (&INFO[261usize], entry),
                91usize => (&INFO[263usize], entry),
                92usize => (&INFO[265usize], entry),
                93usize => (&INFO[267usize], entry),
                94usize => (&INFO[269usize], entry),
                95usize => (&INFO[271usize], entry),
                96usize => (&INFO[273usize], entry),
                97usize => (&INFO[275usize], entry),
                98usize => (&INFO[277usize], entry),
                99usize => (&INFO[280usize], entry),
                100usize => (&INFO[283usize], entry),
                101usize => (&INFO[285usize], entry),
                102usize => (&INFO[287usize], entry),
                103usize => (&INFO[304usize], entry),
                104usize => (&INFO[307usize], entry),
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
                0usize => (&INFO[20usize], entry),
                1usize => (&INFO[21usize], entry),
                2usize => (&INFO[50usize], entry),
                3usize => (&INFO[51usize], entry),
                4usize => (&INFO[52usize], entry),
                5usize => (&INFO[53usize], entry),
                6usize => (&INFO[101usize], entry),
                7usize => (&INFO[141usize], entry),
                8usize => (&INFO[154usize], entry),
                9usize => (&INFO[158usize], entry),
                10usize => (&INFO[161usize], entry),
                11usize => (&INFO[172usize], entry),
                12usize => (&INFO[180usize], entry),
                13usize => (&INFO[187usize], entry),
                14usize => (&INFO[194usize], entry),
                15usize => (&INFO[200usize], entry),
                16usize => (&INFO[210usize], entry),
                17usize => (&INFO[217usize], entry),
                18usize => (&INFO[218usize], entry),
                19usize => (&INFO[279usize], entry),
                20usize => (&INFO[284usize], entry),
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
        #[allow(unused_imports)]
        use api::*;
        self.nominal_counter_offsets
            .iter()
            .enumerate()
            .map(|(idx, entry)| match idx {
                0usize => {
                    let offset = *entry;
                    let variants = <AcceptorTcpStreamDropReason as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[14usize], entries, variants)
                }
                1usize => {
                    let offset = *entry;
                    let variants = <AcceptorPacketDropReason as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[30usize], entries, variants)
                }
                2usize => {
                    let offset = *entry;
                    let variants = <AcceptorTcpIoErrorSource as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[36usize], entries, variants)
                }
                3usize => {
                    let offset = *entry;
                    let variants = <AcceptorPacketDropReason as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[55usize], entries, variants)
                }
                4usize => {
                    let offset = *entry;
                    let variants = <AcceptorStreamPruneReason as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[60usize], entries, variants)
                }
                5usize => {
                    let offset = *entry;
                    let variants = <MaybeBoolCounter as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[162usize], entries, variants)
                }
                6usize => {
                    let offset = *entry;
                    let variants = <MaybeBoolCounter as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[163usize], entries, variants)
                }
                7usize => {
                    let offset = *entry;
                    let variants = <StreamTcpConnectErrorReason as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[165usize], entries, variants)
                }
                8usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[215usize], entries, variants)
                }
                9usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[216usize], entries, variants)
                }
                10usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[220usize], entries, variants)
                }
                11usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[228usize], entries, variants)
                }
                12usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[230usize], entries, variants)
                }
                13usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[232usize], entries, variants)
                }
                14usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[234usize], entries, variants)
                }
                15usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[236usize], entries, variants)
                }
                16usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[239usize], entries, variants)
                }
                17usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[242usize], entries, variants)
                }
                18usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[244usize], entries, variants)
                }
                19usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[246usize], entries, variants)
                }
                20usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[248usize], entries, variants)
                }
                21usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[250usize], entries, variants)
                }
                22usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[258usize], entries, variants)
                }
                23usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[260usize], entries, variants)
                }
                24usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[262usize], entries, variants)
                }
                25usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[264usize], entries, variants)
                }
                26usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[266usize], entries, variants)
                }
                27usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[268usize], entries, variants)
                }
                28usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[270usize], entries, variants)
                }
                29usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[272usize], entries, variants)
                }
                30usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[274usize], entries, variants)
                }
                31usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[276usize], entries, variants)
                }
                32usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[278usize], entries, variants)
                }
                33usize => {
                    let offset = *entry;
                    let variants = <SocketAddress as AsVariant>::VARIANTS;
                    let entries = &self.nominal_counters[offset..offset + variants.len()];
                    (&INFO[281usize], entries, variants)
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
                0usize => (&INFO[1usize], entry),
                1usize => (&INFO[3usize], entry),
                2usize => (&INFO[4usize], entry),
                3usize => (&INFO[5usize], entry),
                4usize => (&INFO[7usize], entry),
                5usize => (&INFO[10usize], entry),
                6usize => (&INFO[11usize], entry),
                7usize => (&INFO[12usize], entry),
                8usize => (&INFO[17usize], entry),
                9usize => (&INFO[19usize], entry),
                10usize => (&INFO[34usize], entry),
                11usize => (&INFO[40usize], entry),
                12usize => (&INFO[41usize], entry),
                13usize => (&INFO[44usize], entry),
                14usize => (&INFO[47usize], entry),
                15usize => (&INFO[49usize], entry),
                16usize => (&INFO[66usize], entry),
                17usize => (&INFO[67usize], entry),
                18usize => (&INFO[69usize], entry),
                19usize => (&INFO[70usize], entry),
                20usize => (&INFO[71usize], entry),
                21usize => (&INFO[72usize], entry),
                22usize => (&INFO[75usize], entry),
                23usize => (&INFO[76usize], entry),
                24usize => (&INFO[78usize], entry),
                25usize => (&INFO[79usize], entry),
                26usize => (&INFO[80usize], entry),
                27usize => (&INFO[81usize], entry),
                28usize => (&INFO[84usize], entry),
                29usize => (&INFO[85usize], entry),
                30usize => (&INFO[86usize], entry),
                31usize => (&INFO[87usize], entry),
                32usize => (&INFO[90usize], entry),
                33usize => (&INFO[91usize], entry),
                34usize => (&INFO[92usize], entry),
                35usize => (&INFO[95usize], entry),
                36usize => (&INFO[96usize], entry),
                37usize => (&INFO[97usize], entry),
                38usize => (&INFO[100usize], entry),
                39usize => (&INFO[103usize], entry),
                40usize => (&INFO[104usize], entry),
                41usize => (&INFO[106usize], entry),
                42usize => (&INFO[107usize], entry),
                43usize => (&INFO[109usize], entry),
                44usize => (&INFO[110usize], entry),
                45usize => (&INFO[112usize], entry),
                46usize => (&INFO[115usize], entry),
                47usize => (&INFO[116usize], entry),
                48usize => (&INFO[118usize], entry),
                49usize => (&INFO[119usize], entry),
                50usize => (&INFO[120usize], entry),
                51usize => (&INFO[121usize], entry),
                52usize => (&INFO[124usize], entry),
                53usize => (&INFO[125usize], entry),
                54usize => (&INFO[126usize], entry),
                55usize => (&INFO[127usize], entry),
                56usize => (&INFO[130usize], entry),
                57usize => (&INFO[131usize], entry),
                58usize => (&INFO[132usize], entry),
                59usize => (&INFO[135usize], entry),
                60usize => (&INFO[136usize], entry),
                61usize => (&INFO[137usize], entry),
                62usize => (&INFO[143usize], entry),
                63usize => (&INFO[144usize], entry),
                64usize => (&INFO[146usize], entry),
                65usize => (&INFO[147usize], entry),
                66usize => (&INFO[149usize], entry),
                67usize => (&INFO[150usize], entry),
                68usize => (&INFO[152usize], entry),
                69usize => (&INFO[155usize], entry),
                70usize => (&INFO[156usize], entry),
                71usize => (&INFO[168usize], entry),
                72usize => (&INFO[170usize], entry),
                73usize => (&INFO[171usize], entry),
                74usize => (&INFO[174usize], entry),
                75usize => (&INFO[176usize], entry),
                76usize => (&INFO[178usize], entry),
                77usize => (&INFO[179usize], entry),
                78usize => (&INFO[182usize], entry),
                79usize => (&INFO[184usize], entry),
                80usize => (&INFO[185usize], entry),
                81usize => (&INFO[186usize], entry),
                82usize => (&INFO[189usize], entry),
                83usize => (&INFO[191usize], entry),
                84usize => (&INFO[192usize], entry),
                85usize => (&INFO[193usize], entry),
                86usize => (&INFO[196usize], entry),
                87usize => (&INFO[198usize], entry),
                88usize => (&INFO[199usize], entry),
                89usize => (&INFO[203usize], entry),
                90usize => (&INFO[205usize], entry),
                91usize => (&INFO[206usize], entry),
                92usize => (&INFO[208usize], entry),
                93usize => (&INFO[209usize], entry),
                94usize => (&INFO[222usize], entry),
                95usize => (&INFO[224usize], entry),
                96usize => (&INFO[225usize], entry),
                97usize => (&INFO[226usize], entry),
                98usize => (&INFO[237usize], entry),
                99usize => (&INFO[240usize], entry),
                100usize => (&INFO[252usize], entry),
                101usize => (&INFO[253usize], entry),
                102usize => (&INFO[256usize], entry),
                103usize => (&INFO[282usize], entry),
                104usize => (&INFO[286usize], entry),
                105usize => (&INFO[288usize], entry),
                106usize => (&INFO[289usize], entry),
                107usize => (&INFO[290usize], entry),
                108usize => (&INFO[291usize], entry),
                109usize => (&INFO[292usize], entry),
                110usize => (&INFO[293usize], entry),
                111usize => (&INFO[294usize], entry),
                112usize => (&INFO[295usize], entry),
                113usize => (&INFO[296usize], entry),
                114usize => (&INFO[297usize], entry),
                115usize => (&INFO[298usize], entry),
                116usize => (&INFO[299usize], entry),
                117usize => (&INFO[300usize], entry),
                118usize => (&INFO[301usize], entry),
                119usize => (&INFO[302usize], entry),
                120usize => (&INFO[303usize], entry),
                121usize => (&INFO[305usize], entry),
                122usize => (&INFO[306usize], entry),
                123usize => (&INFO[308usize], entry),
                124usize => (&INFO[309usize], entry),
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
                0usize => (&INFO[6usize], entry),
                1usize => (&INFO[16usize], entry),
                2usize => (&INFO[22usize], entry),
                3usize => (&INFO[24usize], entry),
                4usize => (&INFO[26usize], entry),
                5usize => (&INFO[28usize], entry),
                6usize => (&INFO[31usize], entry),
                7usize => (&INFO[33usize], entry),
                8usize => (&INFO[38usize], entry),
                9usize => (&INFO[43usize], entry),
                10usize => (&INFO[59usize], entry),
                11usize => (&INFO[62usize], entry),
                12usize => (&INFO[63usize], entry),
                13usize => (&INFO[65usize], entry),
                14usize => (&INFO[74usize], entry),
                15usize => (&INFO[83usize], entry),
                16usize => (&INFO[89usize], entry),
                17usize => (&INFO[99usize], entry),
                18usize => (&INFO[114usize], entry),
                19usize => (&INFO[123usize], entry),
                20usize => (&INFO[129usize], entry),
                21usize => (&INFO[134usize], entry),
                22usize => (&INFO[140usize], entry),
                23usize => (&INFO[159usize], entry),
                24usize => (&INFO[166usize], entry),
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
            ctr_16: AtomicU64::new(0),
            ctr_19: AtomicU64::new(0),
            ctr_21: AtomicU64::new(0),
            ctr_22: AtomicU64::new(0),
            ctr_25: AtomicU64::new(0),
            ctr_27: AtomicU64::new(0),
            ctr_28: AtomicU64::new(0),
            ctr_31: AtomicU64::new(0),
            ctr_34: AtomicU64::new(0),
            ctr_35: AtomicU64::new(0),
            ctr_37: AtomicU64::new(0),
            ctr_39: AtomicU64::new(0),
            ctr_42: AtomicU64::new(0),
            ctr_43: AtomicU64::new(0),
            ctr_46: AtomicU64::new(0),
            ctr_49: AtomicU64::new(0),
            ctr_51: AtomicU64::new(0),
            ctr_52: AtomicU64::new(0),
            ctr_55: AtomicU64::new(0),
            ctr_58: AtomicU64::new(0),
            ctr_61: AtomicU64::new(0),
            ctr_62: AtomicU64::new(0),
            ctr_65: AtomicU64::new(0),
            ctr_66: AtomicU64::new(0),
            ctr_73: AtomicU64::new(0),
            ctr_77: AtomicU64::new(0),
            ctr_80: AtomicU64::new(0),
            ctr_84: AtomicU64::new(0),
            ctr_88: AtomicU64::new(0),
        }
    }
    #[inline]
    fn on_acceptor_tcp_started(&self, meta: &api::EndpointMeta, event: &api::AcceptorTcpStarted) {
        #[allow(unused_imports)]
        use api::*;
        self.count(0usize, 0usize, 1usize);
        self.measure(1usize, 0usize, event.backlog);
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
        self.count(2usize, 1usize, 1usize);
        self.measure(3usize, 1usize, event.pending_streams);
        self.measure(4usize, 2usize, event.slots_idle);
        self.measure(5usize, 3usize, event.slot_utilization);
        self.time(6usize, 0usize, event.processing_duration);
        self.measure(7usize, 4usize, event.max_sojourn_time);
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
        self.count(8usize, 2usize, 1usize);
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
        self.count(9usize, 3usize, 1usize);
        self.measure(10usize, 5usize, event.enqueued);
        self.measure(11usize, 6usize, event.dropped);
        self.measure(12usize, 7usize, event.errored);
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
        self.count(13usize, 4usize, 1usize);
        self.count_nominal(14usize, 0usize, &event.reason);
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
        self.count(15usize, 5usize, 1usize);
        self.time(16usize, 1usize, event.sojourn_time);
        self.measure(17usize, 8usize, event.buffer_len);
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
        self.count(18usize, 6usize, 1usize);
        self.measure(19usize, 9usize, event.payload_len);
        self.count_bool(20usize, 0usize, event.is_fin);
        self.count_bool(21usize, 1usize, event.is_fin_known);
        self.time(22usize, 2usize, event.sojourn_time);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_tcp_tls_started(
        &self,
        meta: &api::EndpointMeta,
        event: &api::AcceptorTcpTlsStarted,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(23usize, 7usize, 1usize);
        self.time(24usize, 3usize, event.sojourn_time);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_tcp_tls_stream_enqueued(
        &self,
        meta: &api::EndpointMeta,
        event: &api::AcceptorTcpTlsStreamEnqueued,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(25usize, 8usize, 1usize);
        self.time(26usize, 4usize, event.sojourn_time);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_tcp_tls_stream_rejected(
        &self,
        meta: &api::EndpointMeta,
        event: &api::AcceptorTcpTlsStreamRejected,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(27usize, 9usize, 1usize);
        self.time(28usize, 5usize, event.sojourn_time);
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
        self.count(29usize, 10usize, 1usize);
        self.count_nominal(30usize, 1usize, &event.reason);
        self.time(31usize, 6usize, event.sojourn_time);
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
        self.count(32usize, 11usize, 1usize);
        self.time(33usize, 7usize, event.sojourn_time);
        self.measure(34usize, 10usize, event.blocked_count);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_tcp_io_error(&self, meta: &api::EndpointMeta, event: &api::AcceptorTcpIoError) {
        #[allow(unused_imports)]
        use api::*;
        self.count(35usize, 12usize, 1usize);
        self.count_nominal(36usize, 2usize, &event.source);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_tcp_socket_sent(
        &self,
        meta: &api::EndpointMeta,
        event: &api::AcceptorTcpSocketSent,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(37usize, 13usize, 1usize);
        self.time(38usize, 8usize, event.sojourn_time);
        self.count(39usize, 14usize, event.blocked_count);
        self.measure(40usize, 11usize, event.blocked_count);
        self.measure(41usize, 12usize, event.payload_len);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_tcp_socket_received(
        &self,
        meta: &api::EndpointMeta,
        event: &api::AcceptorTcpSocketReceived,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(42usize, 15usize, 1usize);
        self.time(43usize, 9usize, event.transfer_time);
        self.measure(44usize, 13usize, event.payload_len);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_udp_started(&self, meta: &api::EndpointMeta, event: &api::AcceptorUdpStarted) {
        #[allow(unused_imports)]
        use api::*;
        self.count(45usize, 16usize, 1usize);
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
        self.count(46usize, 17usize, 1usize);
        self.measure(47usize, 14usize, event.len);
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
        self.count(48usize, 18usize, 1usize);
        self.measure(49usize, 15usize, event.payload_len);
        self.count_bool(50usize, 2usize, event.is_zero_offset);
        self.count_bool(51usize, 3usize, event.is_retransmission);
        self.count_bool(52usize, 4usize, event.is_fin);
        self.count_bool(53usize, 5usize, event.is_fin_known);
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
        self.count(54usize, 19usize, 1usize);
        self.count_nominal(55usize, 3usize, &event.reason);
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
        self.count(56usize, 20usize, 1usize);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_acceptor_udp_io_error(&self, meta: &api::EndpointMeta, event: &api::AcceptorUdpIoError) {
        #[allow(unused_imports)]
        use api::*;
        self.count(57usize, 21usize, 1usize);
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
        self.count(58usize, 22usize, 1usize);
        self.time(59usize, 10usize, event.sojourn_time);
        self.count_nominal(60usize, 4usize, &event.reason);
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
        self.count(61usize, 23usize, 1usize);
        self.time(62usize, 11usize, event.sojourn_time);
        self.time(63usize, 12usize, event.queue_sojourn_time);
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
        self.count(64usize, 24usize, 1usize);
        self.time(
            65usize,
            13usize,
            meta.timestamp.saturating_duration_since(context.start_time),
        );
        context.ctr_16.fetch_add(1, Ordering::Relaxed);
        self.measure(67usize, 17usize, event.provided_len);
        self.count(68usize, 25usize, event.committed_len);
        self.measure(69usize, 18usize, event.committed_len);
        context
            .ctr_19
            .fetch_add(event.committed_len.as_u64(), Ordering::Relaxed);
        self.measure(71usize, 20usize, event.processing_duration);
        context
            .ctr_21
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
        self.count(73usize, 26usize, 1usize);
        self.time(
            74usize,
            14usize,
            meta.timestamp.saturating_duration_since(context.start_time),
        );
        context.ctr_22.fetch_add(1, Ordering::Relaxed);
        self.measure(76usize, 23usize, event.provided_len);
        self.count(77usize, 27usize, event.committed_len);
        self.measure(78usize, 24usize, event.committed_len);
        context
            .ctr_25
            .fetch_add(event.committed_len.as_u64(), Ordering::Relaxed);
        self.measure(80usize, 26usize, event.processing_duration);
        context
            .ctr_27
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
        self.count(82usize, 28usize, 1usize);
        self.time(
            83usize,
            15usize,
            meta.timestamp.saturating_duration_since(context.start_time),
        );
        context.ctr_28.fetch_add(1, Ordering::Relaxed);
        self.measure(85usize, 29usize, event.provided_len);
        self.measure(86usize, 30usize, event.processing_duration);
        context
            .ctr_31
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
        self.count(88usize, 29usize, 1usize);
        self.time(
            89usize,
            16usize,
            meta.timestamp.saturating_duration_since(context.start_time),
        );
        self.measure(90usize, 32usize, event.provided_len);
        self.measure(91usize, 33usize, event.processing_duration);
        context
            .ctr_34
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
        self.count(93usize, 30usize, 1usize);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_stream_write_allocated(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamWriteAllocated,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(94usize, 31usize, 1usize);
        context.ctr_35.fetch_add(1, Ordering::Relaxed);
        self.measure(96usize, 36usize, event.allocated_len);
        context
            .ctr_37
            .fetch_add(event.allocated_len.as_u64(), Ordering::Relaxed);
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
        self.count(98usize, 32usize, 1usize);
        self.time(
            99usize,
            17usize,
            meta.timestamp.saturating_duration_since(context.start_time),
        );
        self.measure(100usize, 38usize, event.buffer_len);
        self.count_bool(101usize, 6usize, event.background);
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
        self.count(102usize, 33usize, 1usize);
        context.ctr_39.fetch_add(1, Ordering::Relaxed);
        self.measure(104usize, 40usize, event.provided_len);
        self.count(105usize, 34usize, event.committed_len);
        self.measure(106usize, 41usize, event.committed_len);
        context
            .ctr_42
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
        self.count(108usize, 35usize, 1usize);
        context.ctr_43.fetch_add(1, Ordering::Relaxed);
        self.measure(110usize, 44usize, event.provided_len);
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
        self.count(111usize, 36usize, 1usize);
        self.measure(112usize, 45usize, event.provided_len);
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
        self.count(113usize, 37usize, 1usize);
        self.time(
            114usize,
            18usize,
            meta.timestamp.saturating_duration_since(context.start_time),
        );
        context.ctr_46.fetch_add(1, Ordering::Relaxed);
        self.measure(116usize, 47usize, event.capacity);
        self.count(117usize, 38usize, event.committed_len);
        self.measure(118usize, 48usize, event.committed_len);
        context
            .ctr_49
            .fetch_add(event.committed_len.as_u64(), Ordering::Relaxed);
        self.measure(120usize, 50usize, event.processing_duration);
        context
            .ctr_51
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
        self.count(122usize, 39usize, 1usize);
        self.time(
            123usize,
            19usize,
            meta.timestamp.saturating_duration_since(context.start_time),
        );
        context.ctr_52.fetch_add(1, Ordering::Relaxed);
        self.measure(125usize, 53usize, event.capacity);
        self.measure(126usize, 54usize, event.processing_duration);
        context
            .ctr_55
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
        self.count(128usize, 40usize, 1usize);
        self.time(
            129usize,
            20usize,
            meta.timestamp.saturating_duration_since(context.start_time),
        );
        self.measure(130usize, 56usize, event.capacity);
        self.measure(131usize, 57usize, event.processing_duration);
        context
            .ctr_58
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
        self.count(133usize, 41usize, 1usize);
        self.time(
            134usize,
            21usize,
            meta.timestamp.saturating_duration_since(context.start_time),
        );
        self.measure(135usize, 59usize, event.capacity);
        self.measure(136usize, 60usize, event.processing_duration);
        context
            .ctr_61
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
        self.count(138usize, 42usize, 1usize);
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
        self.count(139usize, 43usize, 1usize);
        self.time(
            140usize,
            22usize,
            meta.timestamp.saturating_duration_since(context.start_time),
        );
        self.count_bool(141usize, 7usize, event.background);
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
        self.count(142usize, 44usize, 1usize);
        context.ctr_62.fetch_add(1, Ordering::Relaxed);
        self.measure(144usize, 63usize, event.capacity);
        self.count(145usize, 45usize, event.committed_len);
        self.measure(146usize, 64usize, event.committed_len);
        context
            .ctr_65
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
        self.count(148usize, 46usize, 1usize);
        context.ctr_66.fetch_add(1, Ordering::Relaxed);
        self.measure(150usize, 67usize, event.capacity);
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
        self.count(151usize, 47usize, 1usize);
        self.measure(152usize, 68usize, event.capacity);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_stream_decrypt_packet(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamDecryptPacket,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(153usize, 48usize, 1usize);
        self.count_bool(154usize, 8usize, event.decrypted_in_place);
        self.measure(155usize, 69usize, event.forced_copy);
        self.measure(156usize, 70usize, event.required_application_buffer);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_stream_tcp_connect(&self, meta: &api::EndpointMeta, event: &api::StreamTcpConnect) {
        #[allow(unused_imports)]
        use api::*;
        self.count(157usize, 49usize, 1usize);
        self.count_bool(158usize, 9usize, event.error);
        self.time(159usize, 23usize, event.latency);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_stream_connect(&self, meta: &api::EndpointMeta, event: &api::StreamConnect) {
        #[allow(unused_imports)]
        use api::*;
        self.count(160usize, 50usize, 1usize);
        self.count_bool(161usize, 10usize, event.error);
        self.count_nominal(162usize, 5usize, &event.tcp_success);
        self.count_nominal(163usize, 6usize, &event.handshake_success);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_stream_connect_error(&self, meta: &api::EndpointMeta, event: &api::StreamConnectError) {
        #[allow(unused_imports)]
        use api::*;
        self.count(164usize, 51usize, 1usize);
        self.count_nominal(165usize, 7usize, &event.reason);
        self.time(166usize, 24usize, event.latency);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_stream_packet_transmitted(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamPacketTransmitted,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(167usize, 52usize, 1usize);
        self.measure(168usize, 71usize, event.packet_len);
        self.count(169usize, 53usize, event.payload_len);
        self.measure(170usize, 72usize, event.payload_len);
        context
            .ctr_73
            .fetch_add(event.payload_len.as_u64(), Ordering::Relaxed);
        self.count_bool(172usize, 11usize, event.is_retransmission);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_stream_probe_transmitted(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamProbeTransmitted,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(173usize, 54usize, 1usize);
        self.measure(174usize, 74usize, event.packet_len);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_stream_packet_received(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamPacketReceived,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(175usize, 55usize, 1usize);
        self.measure(176usize, 75usize, event.packet_len);
        self.count(177usize, 56usize, event.payload_len);
        self.measure(178usize, 76usize, event.payload_len);
        context
            .ctr_77
            .fetch_add(event.payload_len.as_u64(), Ordering::Relaxed);
        self.count_bool(180usize, 12usize, event.is_retransmission);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_stream_packet_lost(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamPacketLost,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(181usize, 57usize, 1usize);
        self.measure(182usize, 78usize, event.packet_len);
        self.count(183usize, 58usize, event.payload_len);
        self.measure(184usize, 79usize, event.payload_len);
        context
            .ctr_80
            .fetch_add(event.payload_len.as_u64(), Ordering::Relaxed);
        self.measure(186usize, 81usize, event.lifetime);
        self.count_bool(187usize, 13usize, event.is_retransmission);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_stream_packet_acked(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamPacketAcked,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(188usize, 59usize, 1usize);
        self.measure(189usize, 82usize, event.packet_len);
        self.count(190usize, 60usize, event.payload_len);
        self.measure(191usize, 83usize, event.payload_len);
        context
            .ctr_84
            .fetch_add(event.payload_len.as_u64(), Ordering::Relaxed);
        self.measure(193usize, 85usize, event.lifetime);
        self.count_bool(194usize, 14usize, event.is_retransmission);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_stream_packet_spuriously_retransmitted(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamPacketSpuriouslyRetransmitted,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(195usize, 61usize, 1usize);
        self.measure(196usize, 86usize, event.packet_len);
        self.count(197usize, 62usize, event.payload_len);
        self.measure(198usize, 87usize, event.payload_len);
        context
            .ctr_88
            .fetch_add(event.payload_len.as_u64(), Ordering::Relaxed);
        self.count_bool(200usize, 15usize, event.is_retransmission);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_stream_max_data_received(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamMaxDataReceived,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(201usize, 63usize, 1usize);
        self.count(202usize, 64usize, event.increase);
        self.measure(203usize, 89usize, event.increase);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_stream_control_packet_transmitted(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamControlPacketTransmitted,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(204usize, 65usize, 1usize);
        self.measure(205usize, 90usize, event.packet_len);
        self.measure(206usize, 91usize, event.control_data_len);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_stream_control_packet_received(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamControlPacketReceived,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(207usize, 66usize, 1usize);
        self.measure(208usize, 92usize, event.packet_len);
        self.measure(209usize, 93usize, event.control_data_len);
        self.count_bool(210usize, 16usize, event.is_authenticated);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_stream_receiver_errored(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamReceiverErrored,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(211usize, 67usize, 1usize);
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_stream_sender_errored(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamSenderErrored,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(212usize, 68usize, 1usize);
        let _ = context;
        let _ = meta;
        let _ = event;
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
        self.count(213usize, 69usize, 1usize);
        self.measure(66usize, 16usize, context.ctr_16.load(Ordering::Relaxed));
        self.measure(70usize, 19usize, context.ctr_19.load(Ordering::Relaxed));
        self.measure(
            72usize,
            21usize,
            core::time::Duration::from_micros(context.ctr_21.load(Ordering::Relaxed)),
        );
        self.measure(75usize, 22usize, context.ctr_22.load(Ordering::Relaxed));
        self.measure(79usize, 25usize, context.ctr_25.load(Ordering::Relaxed));
        self.measure(
            81usize,
            27usize,
            core::time::Duration::from_micros(context.ctr_27.load(Ordering::Relaxed)),
        );
        self.measure(84usize, 28usize, context.ctr_28.load(Ordering::Relaxed));
        self.measure(
            87usize,
            31usize,
            core::time::Duration::from_micros(context.ctr_31.load(Ordering::Relaxed)),
        );
        self.measure(
            92usize,
            34usize,
            core::time::Duration::from_micros(context.ctr_34.load(Ordering::Relaxed)),
        );
        self.measure(95usize, 35usize, context.ctr_35.load(Ordering::Relaxed));
        self.measure(97usize, 37usize, context.ctr_37.load(Ordering::Relaxed));
        self.measure(103usize, 39usize, context.ctr_39.load(Ordering::Relaxed));
        self.measure(107usize, 42usize, context.ctr_42.load(Ordering::Relaxed));
        self.measure(109usize, 43usize, context.ctr_43.load(Ordering::Relaxed));
        self.measure(115usize, 46usize, context.ctr_46.load(Ordering::Relaxed));
        self.measure(119usize, 49usize, context.ctr_49.load(Ordering::Relaxed));
        self.measure(
            121usize,
            51usize,
            core::time::Duration::from_micros(context.ctr_51.load(Ordering::Relaxed)),
        );
        self.measure(124usize, 52usize, context.ctr_52.load(Ordering::Relaxed));
        self.measure(
            127usize,
            55usize,
            core::time::Duration::from_micros(context.ctr_55.load(Ordering::Relaxed)),
        );
        self.measure(
            132usize,
            58usize,
            core::time::Duration::from_micros(context.ctr_58.load(Ordering::Relaxed)),
        );
        self.measure(
            137usize,
            61usize,
            core::time::Duration::from_micros(context.ctr_61.load(Ordering::Relaxed)),
        );
        self.measure(143usize, 62usize, context.ctr_62.load(Ordering::Relaxed));
        self.measure(147usize, 65usize, context.ctr_65.load(Ordering::Relaxed));
        self.measure(149usize, 66usize, context.ctr_66.load(Ordering::Relaxed));
        self.measure(171usize, 73usize, context.ctr_73.load(Ordering::Relaxed));
        self.measure(179usize, 77usize, context.ctr_77.load(Ordering::Relaxed));
        self.measure(185usize, 80usize, context.ctr_80.load(Ordering::Relaxed));
        self.measure(192usize, 84usize, context.ctr_84.load(Ordering::Relaxed));
        self.measure(199usize, 88usize, context.ctr_88.load(Ordering::Relaxed));
        let _ = context;
        let _ = meta;
        let _ = event;
    }
    #[inline]
    fn on_endpoint_initialized(&self, meta: &api::EndpointMeta, event: &api::EndpointInitialized) {
        #[allow(unused_imports)]
        use api::*;
        self.count(214usize, 70usize, 1usize);
        self.count_nominal(215usize, 8usize, &event.acceptor_addr);
        self.count_nominal(216usize, 9usize, &event.handshake_addr);
        self.count_bool(217usize, 17usize, event.tcp);
        self.count_bool(218usize, 18usize, event.udp);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_dc_connection_timeout(&self, meta: &api::EndpointMeta, event: &api::DcConnectionTimeout) {
        #[allow(unused_imports)]
        use api::*;
        self.count(219usize, 71usize, 1usize);
        self.count_nominal(220usize, 10usize, &event.peer_address);
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
        self.count(221usize, 72usize, 1usize);
        self.measure(222usize, 94usize, event.capacity);
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
        self.count(223usize, 73usize, 1usize);
        self.measure(224usize, 95usize, event.capacity);
        self.measure(225usize, 96usize, event.entries);
        self.measure(226usize, 97usize, event.lifetime);
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
        self.count(227usize, 74usize, 1usize);
        self.count_nominal(228usize, 11usize, &event.peer_address);
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
        self.count(229usize, 75usize, 1usize);
        self.count_nominal(230usize, 12usize, &event.peer_address);
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
        self.count(231usize, 76usize, 1usize);
        self.count_nominal(232usize, 13usize, &event.peer_address);
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
        self.count(233usize, 77usize, 1usize);
        self.count_nominal(234usize, 14usize, &event.peer_address);
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
        self.count(235usize, 78usize, 1usize);
        self.count_nominal(236usize, 15usize, &event.peer_address);
        self.measure(237usize, 98usize, event.age);
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
        self.count(238usize, 79usize, 1usize);
        self.count_nominal(239usize, 16usize, &event.peer_address);
        self.measure(240usize, 99usize, event.age);
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
        self.count(241usize, 80usize, 1usize);
        self.count_nominal(242usize, 17usize, &event.peer_address);
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
        self.count(243usize, 81usize, 1usize);
        self.count_nominal(244usize, 18usize, &event.peer_address);
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
        self.count(245usize, 82usize, 1usize);
        self.count_nominal(246usize, 19usize, &event.peer_address);
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
        self.count(247usize, 83usize, 1usize);
        self.count_nominal(248usize, 20usize, &event.peer_address);
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
        self.count(249usize, 84usize, 1usize);
        self.count_nominal(250usize, 21usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_key_accepted(&self, meta: &api::EndpointMeta, event: &api::KeyAccepted) {
        #[allow(unused_imports)]
        use api::*;
        self.count(251usize, 85usize, 1usize);
        self.measure(252usize, 100usize, event.gap);
        self.measure(253usize, 101usize, event.forward_shift);
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
        self.count(254usize, 86usize, 1usize);
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
        self.count(255usize, 87usize, 1usize);
        self.measure(256usize, 102usize, event.gap);
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
        self.count(257usize, 88usize, 1usize);
        self.count_nominal(258usize, 22usize, &event.peer_address);
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
        self.count(259usize, 89usize, 1usize);
        self.count_nominal(260usize, 23usize, &event.peer_address);
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
        self.count(261usize, 90usize, 1usize);
        self.count_nominal(262usize, 24usize, &event.peer_address);
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
        self.count(263usize, 91usize, 1usize);
        self.count_nominal(264usize, 25usize, &event.peer_address);
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
        self.count(265usize, 92usize, 1usize);
        self.count_nominal(266usize, 26usize, &event.peer_address);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_stale_key_packet_sent(&self, meta: &api::EndpointMeta, event: &api::StaleKeyPacketSent) {
        #[allow(unused_imports)]
        use api::*;
        self.count(267usize, 93usize, 1usize);
        self.count_nominal(268usize, 27usize, &event.peer_address);
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
        self.count(269usize, 94usize, 1usize);
        self.count_nominal(270usize, 28usize, &event.peer_address);
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
        self.count(271usize, 95usize, 1usize);
        self.count_nominal(272usize, 29usize, &event.peer_address);
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
        self.count(273usize, 96usize, 1usize);
        self.count_nominal(274usize, 30usize, &event.peer_address);
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
        self.count(275usize, 97usize, 1usize);
        self.count_nominal(276usize, 31usize, &event.peer_address);
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
        self.count(277usize, 98usize, 1usize);
        self.count_nominal(278usize, 32usize, &event.peer_address);
        self.count_bool(279usize, 19usize, event.hit);
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
        self.count(280usize, 99usize, 1usize);
        self.count_nominal(281usize, 33usize, &event.peer_address);
        self.measure(282usize, 103usize, event.age);
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
        self.count(283usize, 100usize, 1usize);
        self.count_bool(284usize, 20usize, event.hit);
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
        self.count(285usize, 101usize, 1usize);
        self.measure(286usize, 104usize, event.age);
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
        self.count(287usize, 102usize, 1usize);
        self.measure(288usize, 105usize, event.id_entries);
        self.measure(289usize, 106usize, event.id_entries_retired);
        self.measure(290usize, 107usize, event.id_entries_active);
        self.measure(291usize, 108usize, event.id_entries_active_utilization);
        self.measure(292usize, 109usize, event.id_entries_utilization);
        self.measure(293usize, 110usize, event.id_entries_initial_utilization);
        self.measure(294usize, 111usize, event.address_entries);
        self.measure(295usize, 112usize, event.address_entries_active);
        self.measure(296usize, 113usize, event.address_entries_active_utilization);
        self.measure(297usize, 114usize, event.address_entries_retired);
        self.measure(298usize, 115usize, event.address_entries_utilization);
        self.measure(
            299usize,
            116usize,
            event.address_entries_initial_utilization,
        );
        self.measure(300usize, 117usize, event.handshake_requests);
        self.measure(301usize, 118usize, event.handshake_requests_skipped);
        self.measure(302usize, 119usize, event.handshake_lock_duration);
        self.measure(303usize, 120usize, event.duration);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_path_secret_map_id_write_lock(
        &self,
        meta: &api::EndpointMeta,
        event: &api::PathSecretMapIdWriteLock,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(304usize, 103usize, 1usize);
        self.measure(305usize, 121usize, event.acquire);
        self.measure(306usize, 122usize, event.duration);
        let _ = event;
        let _ = meta;
    }
    #[inline]
    fn on_path_secret_map_address_write_lock(
        &self,
        meta: &api::EndpointMeta,
        event: &api::PathSecretMapAddressWriteLock,
    ) {
        #[allow(unused_imports)]
        use api::*;
        self.count(307usize, 104usize, 1usize);
        self.measure(308usize, 123usize, event.acquire);
        self.measure(309usize, 124usize, event.duration);
        let _ = event;
        let _ = meta;
    }
}
