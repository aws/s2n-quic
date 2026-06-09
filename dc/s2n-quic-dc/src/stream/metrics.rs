// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Sojourn time tracking for application frames.
//!
//! Sojourn time is the duration between when an application frame is enqueued
//! into the pipeline (recorded in [`Frame::enqueued_at`]) and when it reaches
//! its final disposition (acknowledged, peer dead, cancelled, etc.).
//!
//! [`SojournMetrics`] collects per-outcome distributions as microsecond
//! summaries.
//!
//! [`ReaderMetrics`] and [`WriterMetrics`] are the top-level per-half metric
//! containers required by the stream constructors.  For now each contains only
//! sojourn histograms, but the structs can be extended with additional counters
//! (e.g. read/write size distributions) in the future.
//!
//! [`Frame::enqueued_at`]: crate::endpoint::frame::Frame::enqueued_at

use crate::{
    counter::{Registry, Summary, Timer, Unit},
    endpoint::frame::FailureReason,
    time::precision::Timestamp,
};

/// Per-outcome sojourn time distributions for application frames.
///
/// Durations are recorded in nanoseconds via [`Summary::record_value`], which
/// [`Unit::Microsecond`] then converts to microseconds at display time.  Each
/// field corresponds to one final disposition of a frame.
#[derive(Clone)]
pub struct SojournMetrics {
    /// Frame acknowledged by the peer.
    pub acked: Summary,
    /// Peer declared dead (PTO reached max idle timeout).
    pub peer_dead: Summary,
    /// Frame cancelled (writer dropped or stream cancelled before ACK).
    pub cancelled: Summary,
    /// Transmission error (retransmission TTL exhausted).
    pub transmission_error: Summary,
    /// Unknown path secret (peer rejected the key).
    pub unknown_path_secret: Summary,
}

impl SojournMetrics {
    /// Create a new bundle of sojourn summaries under the given label prefix.
    ///
    /// All five variants are registered as nominal summaries:
    /// `{label}.sojourn` with variants `acked`, `peer_dead`, etc.
    pub fn new(registry: &Registry, label: &str) -> Self {
        let label = format!("{label}.sojourn");
        Self {
            acked: registry.register_nominal_summary(&label, "acked", Unit::Microsecond),
            peer_dead: registry.register_nominal_summary(&label, "peer_dead", Unit::Microsecond),
            cancelled: registry.register_nominal_summary(&label, "cancelled", Unit::Microsecond),
            transmission_error: registry.register_nominal_summary(
                &label,
                "transmission_error",
                Unit::Microsecond,
            ),
            unknown_path_secret: registry.register_nominal_summary(
                &label,
                "unknown_path_secret",
                Unit::Microsecond,
            ),
        }
    }

    /// Record a sojourn observation.
    ///
    /// `enqueued_at` is the time the frame entered the pipeline.
    /// `completed_at` is the time of final disposition.
    /// `failure` is `None` for a successful ACK, or `Some(reason)` for failures.
    #[inline]
    pub fn record(
        &self,
        enqueued_at: Timestamp,
        completed_at: Timestamp,
        failure: Option<FailureReason>,
    ) {
        let nanos = completed_at.nanos_since(enqueued_at);
        let summary = match failure {
            None => &self.acked,
            Some(FailureReason::PeerDead) => &self.peer_dead,
            Some(FailureReason::Cancelled) => &self.cancelled,
            Some(FailureReason::TransmissionError) => &self.transmission_error,
            Some(FailureReason::UnknownPathSecret) => &self.unknown_path_secret,
        };
        summary.record_value(nanos);
    }
}

/// Metrics for the read half of a stream.
///
/// Passed into [`Reader`](crate::stream::Reader) at construction time. Carries
/// sojourn time histograms and can be extended with additional per-read metrics
/// in the future.
#[derive(Clone)]
pub struct ReaderMetrics {
    pub sojourn: SojournMetrics,
}

impl ReaderMetrics {
    pub fn new(registry: &Registry, label: &str) -> Self {
        Self {
            sojourn: SojournMetrics::new(registry, label),
        }
    }
}

/// Metrics for the write half of a stream.
///
/// Passed into [`Writer`](crate::stream::Writer) at construction time. Carries
/// sojourn time histograms and can be extended with additional per-write metrics
/// in the future.
#[derive(Clone)]
pub struct WriterMetrics {
    pub sojourn: SojournMetrics,
    pub tx_msg_segment_size: Summary,
    pub tx_msg_chunks_per_segment: Summary,
}

impl WriterMetrics {
    pub fn new(registry: &Registry, label: &str) -> Self {
        Self {
            sojourn: SojournMetrics::new(registry, label),
            tx_msg_segment_size: registry.register_summary("tx.msg.segment_size", Unit::Byte),
            tx_msg_chunks_per_segment: registry
                .register_summary("tx.msg.chunks_per_segment", Unit::Count),
        }
    }
}

/// Metrics for client-side stream connect operations.
///
/// `connect_time` records the total wall-clock duration of
/// [`Client::connect`](crate::stream::Client::connect), covering handshake,
/// queue-pair allocation, and stream state setup.
///
/// `alloc_fast` and `alloc_blocked` record the wall-clock duration of the
/// queue-pair allocation step, partitioned by whether the allocation future
/// completed on its first poll (`fast`) or had to suspend waiting for a peer
/// free-list slot (`blocked`).
///
/// `handshake_cached` and `handshake_fresh` record the duration of the PSK
/// handshake step, partitioned by whether a path secret was already present in
/// the cache (`cached`) or had to be negotiated (`fresh`).
///
/// Sampling is disabled on all timers so every connect is recorded.
#[derive(Clone)]
pub struct ClientMetrics {
    pub connect_time: Timer,
    pub alloc_fast: Timer,
    pub alloc_blocked: Timer,
    pub handshake_cached: Timer,
    pub handshake_fresh: Timer,
}

impl ClientMetrics {
    pub fn new(registry: &Registry) -> Self {
        Self {
            connect_time: registry
                .register_timer("stream.client.connect.time")
                .unsampled(),
            alloc_fast: registry
                .register_nominal_timer("stream.client.alloc", "fast")
                .unsampled(),
            alloc_blocked: registry
                .register_nominal_timer("stream.client.alloc", "blocked")
                .unsampled(),
            handshake_cached: registry
                .register_nominal_timer("stream.client.handshake", "cached")
                .unsampled(),
            handshake_fresh: registry
                .register_nominal_timer("stream.client.handshake", "fresh")
                .unsampled(),
        }
    }
}
