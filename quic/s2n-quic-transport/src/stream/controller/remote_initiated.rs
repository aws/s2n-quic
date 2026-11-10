// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    contexts::OnTransmitError,
    sync::{IncrementalValueSync, ValueToFrameWriter},
    transmission,
    transmission::WriteContext,
};
use core::time::Duration;
use s2n_quic_core::{
    ack,
    frame::MaxStreams,
    packet::number::PacketNumber,
    stream::StreamId,
    time::{timer, token_bucket::TokenBucket, Timestamp},
    transport,
    varint::VarInt,
};

//= https://www.rfc-editor.org/rfc/rfc9000#section-4.6
//# An endpoint MUST NOT wait
//# to receive this signal before advertising additional credit, since
//# doing so will mean that the peer will be blocked for at least an
//# entire round trip
// Send a MAX_STREAMS frame whenever 1/10th of the window has been closed
pub const MAX_STREAMS_SYNC_FRACTION: VarInt = VarInt::from_u8(10);

/// The RemoteInitiated controller controls streams initiated by the peer
#[derive(Debug)]
pub(super) struct RemoteInitiated {
    /// The max stream limit specified by the local endpoint.
    ///
    /// Used to calculate updated max_streams_sync value as the peer
    /// closes streams.
    max_local_limit: VarInt,
    /// Maximum cumulative number of streams the remote peer may open.
    /// The advertised MAX_STREAMS value will not exceed this.
    max_total_limit: VarInt,
    /// Responsible for advertising updated max stream frames as the
    /// peer closes streams
    max_streams_sync: IncrementalValueSync<VarInt, MaxStreamsToFrameWriter>,
    opened_streams: VarInt,
    closed_streams: VarInt,
    rtt_refill: TokenBucket,
}

impl RemoteInitiated {
    pub fn new(
        max_local_limit: VarInt,
        max_total_limit: VarInt,
        advertised_limit: VarInt,
        min_rtt: Duration,
    ) -> Self {
        let max_local_limit = max_local_limit.min(max_total_limit);
        // The peer only knows about the advertised limit (from transport parameters).
        // If the local limit was raised post-handshake, the sync baseline must reflect
        // what the peer was actually told, clamped to the effective local limit.
        let synced = advertised_limit.min(max_local_limit);
        let mut controller = Self {
            max_local_limit,
            max_total_limit,
            max_streams_sync: IncrementalValueSync::new(
                max_local_limit,
                synced,
                max_local_limit / MAX_STREAMS_SYNC_FRACTION,
            ),
            opened_streams: VarInt::from_u8(0),
            closed_streams: VarInt::from_u8(0),
            rtt_refill: TokenBucket::builder()
                .with_max(max_local_limit.as_u64())
                .with_refill_interval(min_rtt)
                .with_refill_amount(max_local_limit.as_u64())
                .build(),
        };

        // A post-handshake raise is a one-shot delta with nothing to batch.
        // Force delivery so the peer learns the new limit even when the raise
        // falls below the sync threshold.
        if synced < max_local_limit {
            controller.max_streams_sync.request_delivery();
        }

        controller
    }

    pub fn on_remote_open_stream(&mut self, stream_id: StreamId) -> Result<(), transport::Error> {
        // get the total number of streams that are allowed
        let max_allowed_stream_limit = self.max_streams_sync.latest_value().as_u64();

        // Stream IDs reserve their two low bits for initiator and direction, so
        // the remaining value is the zero-based stream index.
        let opened_stream_count = stream_id.as_varint().as_u64() / 4 + 1;

        if opened_stream_count > max_allowed_stream_limit {
            //= https://www.rfc-editor.org/rfc/rfc9000#section-4.6
            //# Endpoints MUST NOT exceed the limit set by their peer.  An endpoint
            //# that receives a frame with a stream ID exceeding the limit it has
            //# sent MUST treat this as a connection error of type
            //# STREAM_LIMIT_ERROR; see Section 11 for details on error handling.

            //= https://www.rfc-editor.org/rfc/rfc9000#section-19.11
            //# An endpoint MUST terminate a connection
            //# with an error of type STREAM_LIMIT_ERROR if a peer opens more streams
            //# than was permitted.
            return Err(transport::Error::STREAM_LIMIT_ERROR);
        }
        Ok(())
    }

    #[inline]
    pub fn on_open_stream(&mut self) {
        self.opened_streams += 1;
        self.check_integrity();
    }

    #[inline]
    pub fn on_close_stream(&mut self) {
        self.closed_streams += 1;
        self.check_integrity();
    }

    /// Returns the number of streams currently open
    #[inline]
    pub fn open_stream_count(&self) -> VarInt {
        self.opened_streams - self.closed_streams
    }

    #[inline]
    pub fn total_open_stream_count(&self) -> VarInt {
        self.opened_streams
    }

    #[inline]
    pub fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) {
        self.max_streams_sync.on_packet_ack(ack_set)
    }

    #[inline]
    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        self.max_streams_sync.on_packet_loss(ack_set)
    }

    #[inline]
    pub fn on_transmit<W: WriteContext>(
        &mut self,
        stream_id: StreamId,
        context: &mut W,
    ) -> Result<(), OnTransmitError> {
        self.on_timeout(context.current_time());
        self.max_streams_sync.on_transmit(stream_id, context)
    }

    #[inline]
    pub fn on_timeout(&mut self, now: Timestamp) {
        // At the cumulative cap no refill can raise `latest_value`, so skip the
        // token bucket. `max_streams_sync` stays active and still delivers the
        // final `MAX_STREAMS`.
        if self.is_cumulative_cap_reached() {
            return;
        }

        let synced_closed_streams = self.synced_closed_streams();

        let refill = self.closed_streams - synced_closed_streams;

        let refill = self.rtt_refill.take(refill.as_u64(), now);

        // we don't have any refill credits at this time
        if refill == 0 {
            return;
        }

        let refill = VarInt::new(refill).unwrap_or(VarInt::MAX);

        // `synced_closed_streams` subtracts `self.max_local_limit` to get the number of streams
        // that have actually been communicated as closed so we need to add it back to the total
        // here
        let max_streams = synced_closed_streams
            .saturating_add(self.max_local_limit)
            .saturating_add(refill)
            .min(self.max_total_limit);

        self.max_streams_sync.update_latest_value(max_streams);

        // When the cumulative cap is reached, the remaining delta may be smaller
        // than the sync threshold so `update_latest_value` won't request delivery.
        // Force delivery so the peer learns about the final allowance. No later
        // close can grant more credit, so the refill timer is cancelled.
        if self.is_cumulative_cap_reached() {
            self.max_streams_sync.request_delivery();
            self.rtt_refill.cancel();
        }
    }

    pub fn close(&mut self) {
        self.max_streams_sync.stop_sync();
        self.rtt_refill.cancel();
    }

    #[inline]
    pub fn update_min_rtt(&mut self, min_rtt: Duration, now: Timestamp) {
        self.rtt_refill.set_refill_interval(min_rtt);
        self.on_timeout(now);
    }

    #[inline]
    fn check_integrity(&self) {
        if cfg!(debug_assertions) {
            assert!(
                self.closed_streams <= self.opened_streams,
                "Cannot close more streams than previously opened"
            );
            assert!(
                self.open_stream_count() <= self.max_local_limit,
                "Cannot have more incoming streams open concurrently than
                the max_local_limit"
            );
        }
    }

    /// Returns the number of closed streams we've set for the incremental value sync
    #[inline]
    fn synced_closed_streams(&self) -> VarInt {
        self.max_streams_sync.latest_value() - self.max_local_limit
    }

    /// Returns `true` once the `MAX_STREAMS` value to advertise has reached
    /// `max_total_limit`. From then on, closing a stream cannot grant the peer
    /// more credit.
    #[inline]
    fn is_cumulative_cap_reached(&self) -> bool {
        self.max_streams_sync.latest_value() >= self.max_total_limit
    }

    #[cfg(test)]
    pub fn latest_limit(&self) -> VarInt {
        self.max_streams_sync.latest_value()
    }
}

impl timer::Provider for RemoteInitiated {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        self.rtt_refill.timers(query)?;
        Ok(())
    }
}

impl transmission::interest::Provider for RemoteInitiated {
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        use timer::Provider as _;

        self.max_streams_sync.transmission_interest(query)?;

        // check if we need to kick off the token bucket refill timer
        if self.closed_streams > self.synced_closed_streams()
            && !self.is_cumulative_cap_reached()
            && !self.rtt_refill.is_armed()
            && !self.max_streams_sync.is_cancelled()
        {
            query.on_new_data()?;
        }

        Ok(())
    }
}

/// Writes the `MAX_STREAMS` frames based on the stream control window.
#[derive(Debug, Default)]
pub(super) struct MaxStreamsToFrameWriter {}

impl ValueToFrameWriter<VarInt> for MaxStreamsToFrameWriter {
    fn write_value_as_frame<W: WriteContext>(
        &self,
        value: VarInt,
        stream_id: StreamId,
        context: &mut W,
    ) -> Option<PacketNumber> {
        context.write_frame(&MaxStreams {
            stream_type: stream_id.stream_type(),
            maximum_streams: value,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contexts::testing::{MockWriteContext, OutgoingFrameBuffer};
    use core::time::Duration;
    use s2n_quic_core::{
        endpoint,
        frame::Frame,
        packet::number::PacketNumberRange,
        stream::StreamType,
        time::{clock::testing as time, timer::Provider as _, Clock as _},
        transmission::interest::Provider as _,
        transport::parameters::MAX_STREAMS_MAX_VALUE,
    };

    const MIN_RTT: Duration = Duration::from_millis(100);

    /// Verifies the initial advertised MAX_STREAMS equals min(max_local, max_total)
    /// across boundary conditions.
    #[test]
    fn initial_limit_respects_cumulative_cap() {
        for (max_local, max_total, expected, label) in [
            (20, 10, 10, "max_local > max_total: clamps to max_total"),
            (10, 20, 10, "max_local < max_total: uses max_local"),
            (15, 15, 15, "max_local == max_total: both equal"),
            (10, 0, 0, "max_total == 0: no streams allowed"),
            (0, 10, 0, "max_local == 0: no streams allowed"),
        ] {
            let controller = RemoteInitiated::new(
                VarInt::from_u32(max_local),
                VarInt::from_u32(max_total),
                VarInt::from_u32(max_local),
                MIN_RTT,
            );

            assert_eq!(
                controller.latest_limit(),
                VarInt::from_u32(expected),
                "{label}"
            );
        }
    }

    /// Default cumulative cap (MAX_STREAMS_MAX_VALUE) does not constrain the
    /// concurrent window.
    #[test]
    fn default_cumulative_cap_is_unlimited() {
        let max_local = VarInt::from_u32(100);
        let controller = RemoteInitiated::new(max_local, MAX_STREAMS_MAX_VALUE, max_local, MIN_RTT);

        assert_eq!(controller.latest_limit(), max_local);
    }

    #[test]
    fn maximum_stream_limit_accepts_all_valid_stream_ids() {
        let mut controller = RemoteInitiated::new(
            MAX_STREAMS_MAX_VALUE,
            MAX_STREAMS_MAX_VALUE,
            MAX_STREAMS_MAX_VALUE,
            MIN_RTT,
        );
        let last_stream_index = MAX_STREAMS_MAX_VALUE.as_u64() - 1;

        for endpoint in [endpoint::Type::Client, endpoint::Type::Server] {
            for stream_type in [StreamType::Bidirectional, StreamType::Unidirectional] {
                let last_stream = StreamId::nth(endpoint, stream_type, last_stream_index).unwrap();
                assert_eq!(controller.on_remote_open_stream(last_stream), Ok(()));
            }
        }
    }

    /// Refill intervals to advance after the cumulative cap is reached. Enough
    /// for a re-armed token bucket to refill and fire several times.
    const MIN_RTTS_AFTER_CAP: u32 = 5;

    /// Transmits any pending `MAX_STREAMS` frame from `controller` and passes
    /// each written packet to `report`, which is `RemoteInitiated::on_packet_ack`
    /// or `RemoteInitiated::on_packet_loss`. After an ack,
    /// `get_transmission_interest` reflects only refill work. Returns the
    /// `maximum_streams` value of every frame written.
    fn transmit_max_streams(
        controller: &mut RemoteInitiated,
        now: Timestamp,
        report: fn(&mut RemoteInitiated, &PacketNumberRange),
    ) -> Vec<VarInt> {
        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut write_context = MockWriteContext::new(
            now,
            &mut frame_buffer,
            transmission::Constraint::None,
            transmission::Mode::Normal,
            endpoint::Type::Server,
        );
        let stream_id =
            StreamId::nth(endpoint::Type::Client, StreamType::Bidirectional, 0).unwrap();
        controller
            .on_transmit(stream_id, &mut write_context)
            .unwrap();

        let mut sent = Vec::new();
        while let Some(mut written) = write_context.frame_buffer.pop_front() {
            match written.as_frame() {
                Frame::MaxStreams(max_streams) => sent.push(max_streams.maximum_streams),
                frame => panic!("unexpected frame {frame:?}"),
            }
            report(
                controller,
                &PacketNumberRange::new(written.packet_nr, written.packet_nr),
            );
        }
        sent
    }

    /// Expected refill behaviour once the peer closes streams.
    #[derive(Clone, Copy, Debug)]
    enum RefillOutcome {
        /// The advertised `MAX_STREAMS` reaches the cumulative cap. Later closes
        /// leave no refill timer armed and no transmission interest.
        StopsAtCap,
        /// The advertised `MAX_STREAMS` stays below the cumulative cap. Closing
        /// a full window of streams grants the peer a full window of credit.
        ContinuesBelowCap,
    }

    /// The controller is built with an advertised limit, which is below the
    /// effective limit when the limit was raised after the handshake. The peer
    /// then opens and closes streams, and time advances through several minimum
    /// RTTs. Each `MAX_STREAMS` is acknowledged.
    ///
    /// Proves a raised effective limit is sent to the peer before any stream
    /// closes, even when the raise is below the sync threshold. Proves closing
    /// streams at the cumulative cap leaves no refill timer armed and no
    /// transmission interest, whether the cap is reached at construction,
    /// exactly by a refill, or by a clamped refill. Proves refill below the
    /// cumulative cap grants a full window of credit.
    ///
    /// Depends on every stream being closed before the next batch opens, so the
    /// concurrent window equals `latest_limit - total_open_stream_count`.
    #[test]
    fn refill_at_cumulative_cap() {
        use RefillOutcome::{ContinuesBelowCap, StopsAtCap};

        for (max_open, max_total, advertised, expected, label) in [
            (
                10,
                10,
                10,
                StopsAtCap,
                "cumulative cap equals the initial effective limit",
            ),
            (
                10,
                20,
                10,
                StopsAtCap,
                "cumulative cap reached exactly by a full refill",
            ),
            (
                10,
                25,
                10,
                StopsAtCap,
                "cumulative cap reached by a clamped refill",
            ),
            (
                20,
                21,
                20,
                StopsAtCap,
                "sub-threshold final refill reaches the cumulative cap",
            ),
            (
                10,
                1000,
                10,
                ContinuesBelowCap,
                "full refill below the cumulative cap",
            ),
            (
                105,
                1000,
                100,
                ContinuesBelowCap,
                "raise below the sync threshold, below the cumulative cap",
            ),
            (
                105,
                105,
                100,
                StopsAtCap,
                "raise below the sync threshold reaches the cumulative cap",
            ),
        ] {
            let max_open = VarInt::from_u32(max_open);
            let max_total = VarInt::from_u32(max_total);
            let advertised = VarInt::from_u32(advertised);

            time::reset();
            let mut clock = time::Clock::default();
            let mut controller = RemoteInitiated::new(max_open, max_total, advertised, MIN_RTT);
            let mut sent = Vec::new();

            // The peer only knows `advertised`, so a higher effective limit must
            // be sent before the peer can use it.
            let effective_limit = max_open.min(max_total);
            if advertised < effective_limit {
                assert_eq!(
                    controller.get_transmission_interest(),
                    transmission::Interest::NewData,
                    "{label}: transmission interest for the raised effective limit"
                );
                let raised = transmit_max_streams(
                    &mut controller,
                    clock.get_time(),
                    RemoteInitiated::on_packet_ack,
                );
                assert_eq!(
                    raised,
                    [effective_limit],
                    "{label}: MAX_STREAMS for the raised effective limit"
                );
                sent.extend(raised);
            }

            match expected {
                ContinuesBelowCap => {
                    let permitted =
                        controller.latest_limit() - controller.total_open_stream_count();
                    for _ in 0..*permitted {
                        controller.on_open_stream();
                    }
                    for _ in 0..*permitted {
                        controller.on_close_stream();
                    }
                    assert_eq!(
                        controller.get_transmission_interest(),
                        transmission::Interest::NewData,
                        "{label}: refill interest after closing streams"
                    );

                    let refilled = controller.latest_limit() + permitted;
                    assert!(
                        refilled < max_total,
                        "{label}: refill stays below the cumulative cap"
                    );
                    clock.inc_by(MIN_RTT);
                    controller.on_timeout(clock.get_time());
                    assert_eq!(controller.latest_limit(), refilled, "{label}");
                    assert_eq!(
                        transmit_max_streams(
                            &mut controller,
                            clock.get_time(),
                            RemoteInitiated::on_packet_ack
                        ),
                        [refilled],
                        "{label}: MAX_STREAMS for the refilled limit"
                    );
                }
                StopsAtCap => {
                    while controller.total_open_stream_count() < max_total {
                        let permitted =
                            controller.latest_limit() - controller.total_open_stream_count();
                        for _ in 0..*permitted {
                            controller.on_open_stream();
                        }
                        for _ in 0..*permitted {
                            controller.on_close_stream();
                        }

                        clock.inc_by(MIN_RTT);
                        controller.on_timeout(clock.get_time());
                        if controller.latest_limit() < max_total {
                            assert!(
                                controller.is_armed(),
                                "{label}: refill timer armed below the cumulative cap"
                            );
                        } else if advertised < max_total && !sent.contains(&max_total) {
                            assert_eq!(
                                controller.get_transmission_interest(),
                                transmission::Interest::NewData,
                                "{label}: transmission interest for the final MAX_STREAMS"
                            );
                        }
                        sent.extend(transmit_max_streams(
                            &mut controller,
                            clock.get_time(),
                            RemoteInitiated::on_packet_ack,
                        ));
                    }
                    assert_eq!(controller.latest_limit(), max_total, "{label}");

                    for _ in 0..MIN_RTTS_AFTER_CAP {
                        clock.inc_by(MIN_RTT);
                        controller.on_timeout(clock.get_time());
                        assert!(
                            !controller.is_armed(),
                            "{label}: refill timer armed after the cumulative cap"
                        );
                        assert_eq!(
                            controller.get_transmission_interest(),
                            transmission::Interest::None,
                            "{label}: transmission interest after the cumulative cap"
                        );
                        assert_eq!(controller.latest_limit(), max_total, "{label}");
                    }

                    controller.update_min_rtt(MIN_RTT * 2, clock.get_time());
                    assert!(
                        !controller.is_armed(),
                        "{label}: refill timer armed after a min RTT update at the cumulative cap"
                    );
                    assert_eq!(
                        controller.get_transmission_interest(),
                        transmission::Interest::None,
                        "{label}: transmission interest after a min RTT update at the cumulative cap"
                    );

                    // The peer already knows the cap when it was advertised in
                    // the transport parameters. Otherwise it is acknowledged exactly once.
                    let expected_cap_frames = usize::from(advertised < max_total);
                    assert_eq!(
                        sent.iter().filter(|value| **value == max_total).count(),
                        expected_cap_frames,
                        "{label}: MAX_STREAMS for the cumulative cap, sent {sent:?}"
                    );
                }
            }
        }
    }

    /// The peer opens and closes streams until the advertised `MAX_STREAMS`
    /// reaches the cumulative cap. The packet carrying that final frame is
    /// reported lost instead of acknowledged.
    ///
    /// Proves the lost `MAX_STREAMS` at the cumulative cap is retransmitted
    /// exactly once, the refill timer stays disarmed through the loss and
    /// retransmit, and transmission interest is gone after the resend is
    /// acknowledged.
    #[test]
    fn cap_max_streams_retransmitted_after_loss() {
        for (max_open, max_total, label) in [
            (10, 20, "cumulative cap reached exactly by a full refill"),
            (10, 25, "cumulative cap reached by a clamped refill"),
        ] {
            let max_open = VarInt::from_u32(max_open);
            let max_total = VarInt::from_u32(max_total);

            time::reset();
            let mut clock = time::Clock::default();
            let mut controller = RemoteInitiated::new(max_open, max_total, max_open, MIN_RTT);

            // Drive the controller to the cumulative cap by opening and closing
            // full windows, acking every intermediate MAX_STREAMS.
            while controller.total_open_stream_count() < max_total {
                let permitted = controller.latest_limit() - controller.total_open_stream_count();
                for _ in 0..*permitted {
                    controller.on_open_stream();
                }
                for _ in 0..*permitted {
                    controller.on_close_stream();
                }

                clock.inc_by(MIN_RTT);
                controller.on_timeout(clock.get_time());

                if controller.latest_limit() < max_total {
                    transmit_max_streams(
                        &mut controller,
                        clock.get_time(),
                        RemoteInitiated::on_packet_ack,
                    );
                }
            }
            assert_eq!(controller.latest_limit(), max_total, "{label}");

            // The frame carrying the cumulative cap is reported lost.
            let lost = transmit_max_streams(
                &mut controller,
                clock.get_time(),
                RemoteInitiated::on_packet_loss,
            );
            assert_eq!(lost, [max_total], "{label}: lost MAX_STREAMS value");
            assert!(
                !controller.is_armed(),
                "{label}: refill timer must not be armed after losing the cap frame"
            );

            // The lost frame is retransmitted exactly once.
            let retransmitted = transmit_max_streams(
                &mut controller,
                clock.get_time(),
                RemoteInitiated::on_packet_ack,
            );
            assert_eq!(
                retransmitted,
                [max_total],
                "{label}: retransmitted MAX_STREAMS value"
            );
            assert!(
                !controller.is_armed(),
                "{label}: refill timer must not be armed after retransmitting the cap frame"
            );

            // After acking the retransmit, no transmission interest remains.
            assert_eq!(
                controller.get_transmission_interest(),
                transmission::Interest::None,
                "{label}: transmission interest after retransmit is acknowledged"
            );
        }
    }
}
