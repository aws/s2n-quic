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
    counter::{Counter, Registry, Summary, Timer, Unit},
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

/// Per-stream receive-side flow-control counters.
///
/// The reader owns the advertised receive window (`remote_max_data` on the peer's writer): it
/// grows the window by acquiring recv-pool credit and sending MAX_DATA frames in
/// [`Reader::maybe_send_max_data`]. When the window stops advancing, the peer writer stalls with
/// no transport-level error — these counters expose *why* the window stopped growing, which is
/// otherwise invisible.
///
/// Only the two failure branches are counted, not the benign early-returns
/// (below-threshold / delta-zero): `maybe_send_max_data` is called on every read-poll, including
/// empty busy-poll Pending polls, so counting the common idle path would both swamp the signal
/// and add a per-poll atomic. These two fire only on a genuine window-growth *attempt* that
/// couldn't be fully satisfied — a bounded, meaningful event.
///
/// [`Reader::maybe_send_max_data`]: crate::stream::recv::Reader
#[derive(Clone)]
pub struct ReaderFlowMetrics {
    /// The reader wanted to grow the window but the recv credit pool `poll_acquire` parked: only a
    /// partial (or zero) extension could be advertised this turn. **This is the recv-side stall
    /// signal.** Nonzero is normal under pool contention; the diagnostic is a *sustained* rate, or
    /// this pinned high while the reader makes no read progress — a reader stuck here cannot open
    /// the window, which starves the peer writer.
    pub max_data_credit_parked: Counter,
    /// The reader wanted to grow the window but collected zero credit (unbacked exhausted, no
    /// prior grant, fresh acquire parked) so advertised nothing at all — the window did not move
    /// despite the writer asking for more room. A transient blip is normal (the next poll
    /// re-tries when the distributor delivers); sustained growth is the hard recv-side stall.
    pub max_data_granted_zero: Counter,
}

impl ReaderFlowMetrics {
    pub fn new(registry: &Registry) -> Self {
        Self {
            max_data_credit_parked: registry
                .register_nominal("rx.flow.max_data_not_sent", "credit_parked"),
            max_data_granted_zero: registry
                .register_nominal("rx.flow.max_data_not_sent", "granted_zero"),
        }
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
    pub flow: ReaderFlowMetrics,
}

impl ReaderMetrics {
    pub fn new(registry: &Registry, label: &str) -> Self {
        Self {
            sojourn: SojournMetrics::new(registry, label),
            flow: ReaderFlowMetrics::new(registry),
        }
    }
}

/// Per-stream send-side flow-control counters.
///
/// The writer's per-poll send budget is `min(flow_budget(), pending_credits)`, where
/// `flow_budget() = remote_max_data − next_offset` is the room left in the peer's advertised
/// receive window. When that window is exhausted the writer produces no frame and parks — but
/// because `poll_acquire_credits` short-circuits on a zero `want`, the credit pool sees an
/// *idle* writer, not a blocked one. These counters make the window-starved stall observable.
#[derive(Clone)]
pub struct WriterFlowMetrics {
    /// The writer had buffered data to send but the peer's advertised window was fully consumed
    /// (`flow_budget() == 0`), so it parked without emitting a data frame. **This is the send-side
    /// stall signal.** A fast writer routinely outrunning the window makes this nonzero in healthy
    /// operation — the diagnostic is a *sustained* rate, or this climbing while throughput is flat.
    /// Pair with `rx.flow.max_data_not_sent` on the peer to see why the window isn't growing.
    pub window_blocked: Counter,
    /// Standalone `QueueDataBlocked` frames emitted (the cold path where no data frame carries the
    /// in-band blocked bit). Climbing here while the peer's `rx.flow.max_data_not_sent` also climbs
    /// is the smoking gun for a wedged window.
    pub data_blocked_sent: Counter,
}

impl WriterFlowMetrics {
    pub fn new(registry: &Registry) -> Self {
        Self {
            window_blocked: registry.register("tx.flow.window_blocked"),
            data_blocked_sent: registry.register("tx.flow.data_blocked_sent"),
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
    pub flow: WriterFlowMetrics,
}

impl WriterMetrics {
    pub fn new(registry: &Registry, label: &str) -> Self {
        Self {
            sojourn: SojournMetrics::new(registry, label),
            tx_msg_segment_size: registry.register_summary("tx.msg.segment_size", Unit::Byte),
            tx_msg_chunks_per_segment: registry
                .register_summary("tx.msg.chunks_per_segment", Unit::Count),
            flow: WriterFlowMetrics::new(registry),
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
