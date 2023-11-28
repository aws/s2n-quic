// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use bolero::{check, generator::*};
use futures_test::task::new_count_waker;
use hashbrown::HashSet;
use s2n_quic_core::{
    recovery::DEFAULT_INITIAL_RTT,
    time::{testing::Clock, Clock as _},
    varint::VarInt,
};
use std::ops::RangeInclusive;

#[derive(Debug)]
struct Oracle {
    local_endpoint_type: endpoint::Type,
    initial_local_limits: InitialFlowControlLimits,
    initial_remote_limits: InitialFlowControlLimits,

    max_remote_bidi_opened_nth_idx: Option<u64>,
    max_remote_uni_opened_nth_idx: Option<u64>,
    max_local_bidi_opened_nth_idx: Option<u64>,
    max_local_uni_opened_nth_idx: Option<u64>,

    remote_bidi_open_idx_set: HashSet<u64>,
    remote_uni_open_idx_set: HashSet<u64>,
    local_bidi_open_idx_set: HashSet<u64>,
    local_uni_open_idx_set: HashSet<u64>,
}

impl Oracle {
    fn can_open_remote(&self, stream_type: StreamType, nth_idx: u64) -> bool {
        // the count is +1 since streams are 0-indexed
        let nth_cnt = nth_idx + 1;
        let limit = self.remote_limit(stream_type);

        nth_cnt <= limit
    }

    fn on_open_stream(
        &mut self,
        stream_initiator: endpoint::Type,
        stream_type: StreamType,
        nth_idx: u64,
    ) {
        match (stream_initiator == self.local_endpoint_type, stream_type) {
            (true, StreamType::Bidirectional) => self.max_local_bidi_opened_nth_idx = Some(nth_idx),
            (true, StreamType::Unidirectional) => self.max_local_uni_opened_nth_idx = Some(nth_idx),
            (false, StreamType::Bidirectional) => {
                self.max_remote_bidi_opened_nth_idx = Some(nth_idx)
            }
            (false, StreamType::Unidirectional) => {
                self.max_remote_uni_opened_nth_idx = Some(nth_idx)
            }
        };

        match (stream_initiator == self.local_endpoint_type, stream_type) {
            (true, StreamType::Bidirectional) => self.local_bidi_open_idx_set.insert(nth_idx),
            (true, StreamType::Unidirectional) => self.local_uni_open_idx_set.insert(nth_idx),
            (false, StreamType::Bidirectional) => self.remote_bidi_open_idx_set.insert(nth_idx),
            (false, StreamType::Unidirectional) => self.remote_uni_open_idx_set.insert(nth_idx),
        };
    }

    // Returns the range of stream ids which can be opened.
    //
    // None is returned if no streams can be opened. None is returned if
    // max_opened_nth_idx stream is > than the nth_idx stream requested. This
    // is because opening a higher stream also opens the lower streams and we
    // do not handle opening a closed stream.
    fn open_stream_range(
        &self,
        stream_initiator: endpoint::Type,
        stream_type: StreamType,
        nth_idx: u64,
    ) -> Option<RangeInclusive<u64>> {
        let max_opened_nth_idx = match (stream_initiator == self.local_endpoint_type, stream_type) {
            (true, StreamType::Bidirectional) => self.max_local_bidi_opened_nth_idx,
            (true, StreamType::Unidirectional) => self.max_local_uni_opened_nth_idx,
            (false, StreamType::Bidirectional) => self.max_remote_bidi_opened_nth_idx,
            (false, StreamType::Unidirectional) => self.max_remote_uni_opened_nth_idx,
        };

        let stream_nth_idx_iter = if let Some(max_opened_nth_idx) = max_opened_nth_idx {
            // idx already opened.. return
            if max_opened_nth_idx >= nth_idx {
                return None;
            }

            // +1 to get the next stream to open
            max_opened_nth_idx + 1..=nth_idx
        } else {
            0..=nth_idx
        };

        Some(stream_nth_idx_iter)
    }

    fn can_close(
        &self,
        stream_initiator: endpoint::Type,
        stream_type: StreamType,
        nth_idx: u64,
    ) -> bool {
        match (stream_initiator == self.local_endpoint_type, stream_type) {
            (true, StreamType::Bidirectional) => self.local_bidi_open_idx_set.contains(&nth_idx),
            (true, StreamType::Unidirectional) => self.local_uni_open_idx_set.contains(&nth_idx),
            (false, StreamType::Bidirectional) => self.remote_bidi_open_idx_set.contains(&nth_idx),
            (false, StreamType::Unidirectional) => self.remote_uni_open_idx_set.contains(&nth_idx),
        }
    }

    fn on_close_stream(
        &mut self,
        stream_initiator: endpoint::Type,
        stream_type: StreamType,
        nth_idx: u64,
    ) {
        match (stream_initiator == self.local_endpoint_type, stream_type) {
            (true, StreamType::Bidirectional) => {
                self.local_bidi_open_idx_set.take(&nth_idx).unwrap();
            }
            (true, StreamType::Unidirectional) => {
                self.local_uni_open_idx_set.take(&nth_idx).unwrap();
            }
            (false, StreamType::Bidirectional) => {
                self.remote_bidi_open_idx_set.take(&nth_idx).unwrap();
                self.initial_local_limits
                    .max_open_remote_bidirectional_streams += 1;
            }
            (false, StreamType::Unidirectional) => {
                self.remote_uni_open_idx_set.take(&nth_idx).unwrap();
                self.initial_local_limits
                    .max_open_remote_unidirectional_streams += 1;
            }
        };
    }

    fn remote_limit(&self, stream_type: StreamType) -> u64 {
        match stream_type {
            StreamType::Bidirectional => {
                self.initial_local_limits
                    .max_open_remote_bidirectional_streams
            }
            StreamType::Unidirectional => {
                self.initial_local_limits
                    .max_open_remote_unidirectional_streams
            }
        }
        .as_u64()
    }

    fn open_streams_count(&self, stream_initiator: endpoint::Type, stream_type: StreamType) -> u64 {
        match (stream_initiator == self.local_endpoint_type, stream_type) {
            (true, StreamType::Bidirectional) => self.local_bidi_open_idx_set.len() as u64,
            (true, StreamType::Unidirectional) => self.local_uni_open_idx_set.len() as u64,
            (false, StreamType::Bidirectional) => self.remote_bidi_open_idx_set.len() as u64,
            (false, StreamType::Unidirectional) => self.remote_uni_open_idx_set.len() as u64,
        }
    }

    fn on_max_stream_local(&mut self, maximum_streams: u8, direction: StreamDirection) {
        match direction {
            StreamDirection::LocalInitiatedBidirectional => {
                if self
                    .initial_remote_limits
                    .max_open_remote_bidirectional_streams
                    .as_u64()
                    < maximum_streams.into()
                {
                    self.initial_remote_limits
                        .max_open_remote_bidirectional_streams = VarInt::from_u8(maximum_streams);
                }
            }
            StreamDirection::LocalInitiatedUnidirectional => {
                if self
                    .initial_remote_limits
                    .max_open_remote_unidirectional_streams
                    .as_u64()
                    < maximum_streams.into()
                {
                    self.initial_remote_limits
                        .max_open_remote_unidirectional_streams = VarInt::from_u8(maximum_streams);
                }
            }
            StreamDirection::RemoteInitiatedBidirectional
            | StreamDirection::RemoteInitiatedUnidirectional => {
                panic!("should only be called for local endpoints")
            }
        };
    }
}

#[derive(Debug)]
struct Model {
    oracle: Oracle,
    subject: Controller,
}

impl Model {
    fn new(local_endpoint_type: endpoint::Type, limits: Limits) -> Self {
        let (initial_local_limits, initial_remote_limits, stream_limits) =
            limits.as_controller_limits();

        Model {
            oracle: Oracle {
                local_endpoint_type,
                initial_local_limits,
                initial_remote_limits,
                max_remote_bidi_opened_nth_idx: None,
                max_remote_uni_opened_nth_idx: None,
                max_local_bidi_opened_nth_idx: None,
                max_local_uni_opened_nth_idx: None,
                remote_bidi_open_idx_set: HashSet::new(),
                remote_uni_open_idx_set: HashSet::new(),
                local_bidi_open_idx_set: HashSet::new(),
                local_uni_open_idx_set: HashSet::new(),
            },
            subject: Controller::new(
                local_endpoint_type,
                initial_remote_limits,
                initial_local_limits,
                stream_limits,
                DEFAULT_INITIAL_RTT,
            ),
        }
    }

    pub fn apply(&mut self, operation: &Operation) {
        match operation {
            Operation::OpenRemoteBidi { nth_idx } => {
                self.on_open_remote(*nth_idx as u64, StreamType::Bidirectional)
            }
            Operation::OpenRemoteUni { nth_idx } => {
                self.on_open_remote(*nth_idx as u64, StreamType::Unidirectional)
            }
            Operation::OpenLocalBidi { nth_idx } => {
                self.on_open_local(*nth_idx as u64, StreamType::Bidirectional)
            }
            Operation::OpenLocalUni { nth_idx } => {
                self.on_open_local(*nth_idx as u64, StreamType::Unidirectional)
            }
            Operation::CloseRemoteBidi { nth_idx } => {
                self.on_close_remote(*nth_idx as u64, StreamType::Bidirectional)
            }
            Operation::CloseRemoteUni { nth_idx } => {
                self.on_close_remote(*nth_idx as u64, StreamType::Unidirectional)
            }
            Operation::CloseLocalBidi { nth_idx } => {
                self.on_close_local(*nth_idx as u64, StreamType::Bidirectional)
            }
            Operation::CloseLocalUni { nth_idx } => {
                self.on_close_local(*nth_idx as u64, StreamType::Unidirectional)
            }
            Operation::MaxStreamLocalBidi { maximum_streams } => self.on_max_stream_local(
                *maximum_streams,
                StreamDirection::LocalInitiatedBidirectional,
            ),
            Operation::MaxStreamLocalUni { maximum_streams } => self.on_max_stream_local(
                *maximum_streams,
                StreamDirection::LocalInitiatedUnidirectional,
            ),
        }
    }

    fn on_max_stream_local(&mut self, maximum_streams: u8, direction: StreamDirection) {
        let stream_type = match direction {
            StreamDirection::LocalInitiatedBidirectional => StreamType::Bidirectional,
            StreamDirection::LocalInitiatedUnidirectional => StreamType::Unidirectional,
            StreamDirection::RemoteInitiatedBidirectional
            | StreamDirection::RemoteInitiatedUnidirectional => panic!("dont expect"),
        };
        let frame = MaxStreams {
            stream_type,
            maximum_streams: VarInt::from_u8(maximum_streams),
        };
        self.subject.on_max_streams(&frame);
        self.oracle.on_max_stream_local(maximum_streams, direction);
    }

    fn on_open_local(&mut self, nth_idx: u64, stream_type: StreamType) {
        let (waker, _wake_counter) = new_count_waker();
        let mut token = connection::OpenToken::new();

        let stream_initiator = self.oracle.local_endpoint_type;

        //-------------
        let stream_nth_idx_iter =
            match self
                .oracle
                .open_stream_range(stream_initiator, stream_type, nth_idx)
            {
                Some(val) => val,
                None => return,
            };

        for stream_nth_idx in stream_nth_idx_iter {
            let can_open = self.can_open_local(stream_type);

            let res = self.subject.poll_open_local_stream(
                stream_type,
                &mut token,
                &Context::from_waker(&waker),
            );

            if can_open {
                assert!(res.is_ready());
                self.oracle
                    .on_open_stream(stream_initiator, stream_type, stream_nth_idx);
            } else {
                assert!(res.is_pending())
            }
        }
    }

    fn on_open_remote(&mut self, nth_idx: u64, stream_type: StreamType) {
        let stream_initiator = self.oracle.local_endpoint_type.peer_type();

        //-------------
        let stream_nth_idx_iter =
            match self
                .oracle
                .open_stream_range(stream_initiator, stream_type, nth_idx)
            {
                Some(val) => val,
                None => return,
            };

        let start_stream =
            StreamId::nth(stream_initiator, stream_type, *stream_nth_idx_iter.start()).unwrap();
        let end_stream =
            StreamId::nth(stream_initiator, stream_type, *stream_nth_idx_iter.end()).unwrap();

        let stream_iter = StreamIter::new(start_stream, end_stream);
        let res = self.subject.on_open_remote_stream(stream_iter);

        if self.can_open_remote(stream_type, nth_idx) {
            for stream_nth_idx in stream_nth_idx_iter {
                self.oracle
                    .on_open_stream(stream_initiator, stream_type, stream_nth_idx);
            }
            res.unwrap();
        } else {
            res.expect_err("limits violated");
        }
    }

    fn on_close_local(&mut self, nth_idx: u64, stream_type: StreamType) {
        let stream_initiator = self.oracle.local_endpoint_type;

        //-------------
        if !self
            .oracle
            .can_close(stream_initiator, stream_type, nth_idx)
        {
            return;
        }

        self.oracle
            .on_close_stream(stream_initiator, stream_type, nth_idx);
        let stream_id = StreamId::nth(stream_initiator, stream_type, nth_idx).unwrap();
        self.subject.on_close_stream(stream_id);
    }

    fn on_close_remote(&mut self, nth_idx: u64, stream_type: StreamType) {
        let stream_initiator = self.oracle.local_endpoint_type.peer_type();

        //-------------
        if !self
            .oracle
            .can_close(stream_initiator, stream_type, nth_idx)
        {
            return;
        }

        self.oracle
            .on_close_stream(stream_initiator, stream_type, nth_idx);
        let stream_id = StreamId::nth(stream_initiator, stream_type, nth_idx).unwrap();
        self.subject.on_close_stream(stream_id);
    }

    fn can_open_local(&self, stream_type: StreamType) -> bool {
        let available_stream_capacity = self
            .subject
            .available_local_initiated_stream_capacity(stream_type);

        available_stream_capacity > VarInt::from_u8(0)
    }

    fn can_open_remote(&self, stream_type: StreamType, nth_idx: u64) -> bool {
        self.oracle.can_open_remote(stream_type, nth_idx)
    }

    /// Check that the subject and oracle match.
    pub fn invariants(&self) {
        // ---------
        let mut stream_initiator = self.oracle.local_endpoint_type.peer_type();
        let mut stream_type = StreamType::Bidirectional;
        assert_eq!(
            self.subject.remote_bidi_controller.open_stream_count(),
            self.oracle
                .open_streams_count(stream_initiator, stream_type)
        );
        assert_eq!(
            self.subject
                .remote_initiated_max_streams_latest_value(stream_type)
                .as_u64(),
            self.oracle.remote_limit(stream_type)
        );

        // ---------
        stream_initiator = self.oracle.local_endpoint_type.peer_type();
        stream_type = StreamType::Unidirectional;
        assert_eq!(
            self.subject.remote_uni_controller.open_stream_count(),
            self.oracle
                .open_streams_count(stream_initiator, stream_type)
        );
        assert_eq!(
            self.subject
                .remote_initiated_max_streams_latest_value(stream_type)
                .as_u64(),
            self.oracle.remote_limit(stream_type)
        );

        // ---------
        stream_initiator = self.oracle.local_endpoint_type;
        stream_type = StreamType::Bidirectional;
        assert_eq!(
            self.subject.local_bidi_controller.open_stream_count(),
            self.oracle
                .open_streams_count(stream_initiator, stream_type)
        );
        self.subject.local_bidi_controller.check_integrity();

        // ---------
        stream_initiator = self.oracle.local_endpoint_type;
        stream_type = StreamType::Unidirectional;
        assert_eq!(
            self.subject.local_uni_controller.open_stream_count(),
            self.oracle
                .open_streams_count(stream_initiator, stream_type)
        );
        self.subject.local_uni_controller.check_integrity();
    }
}

#[derive(Debug, TypeGenerator)]
enum Operation {
    // max_local_limit: max_remote_uni_stream (initial_local_limits)
    // transmit: max_streams
    OpenRemoteBidi { nth_idx: u8 },
    CloseRemoteBidi { nth_idx: u8 },

    // max_local_limit: max_remote_uni_stream (initial_local_limits)
    // transmit: max_streams
    OpenRemoteUni { nth_idx: u8 },
    CloseRemoteUni { nth_idx: u8 },

    // max_local_limit: max_local_bidi_stream
    // peer_stream_limit: peer_max_bidi_stream (initial_remote_limits)
    //
    // limits: max_local_bidi_stream.min(peer_max_bidi_stream)
    // transmit: streams_blocked
    OpenLocalBidi { nth_idx: u8 },
    CloseLocalBidi { nth_idx: u8 },
    MaxStreamLocalBidi { maximum_streams: u8 },

    // max_local_limit: max_local_uni_stream
    // peer_stream_limit: peer_max_uni_stream (initial_remote_limits)
    //
    // limits: max_local_uni_stream.min(peer_max_uni_stream)
    // transmit: streams_blocked
    OpenLocalUni { nth_idx: u8 },
    CloseLocalUni { nth_idx: u8 },
    MaxStreamLocalUni { maximum_streams: u8 },
}

#[derive(Debug, TypeGenerator, Clone, Copy)]
struct Limits {
    // OpenRemoteBidi (initial_local_limits)
    initial_local_max_remote_bidi: u8,

    // OpenRemoteUni (initial_local_limits)
    initial_local_max_remote_uni: u8,

    // OpenLocalBidi (initial_remote_limits)
    //  initial_remote_max_local_bidi.min(app_max_local_bidi)
    initial_remote_max_local_bidi: u8,
    app_max_local_bidi: u8,

    // OpenLocalUni (initial_remote_limits)
    //  initial_remote_max_local_uni.min(app_max_local_uni)
    initial_remote_max_local_uni: u8,
    app_max_local_uni: u8,
}

impl Limits {
    fn as_controller_limits(
        &self,
    ) -> (
        InitialFlowControlLimits,
        InitialFlowControlLimits,
        stream::Limits,
    ) {
        let mut initial_local_limits = InitialFlowControlLimits::default();
        let mut initial_remote_limits = InitialFlowControlLimits::default();
        let stream_limits = stream::Limits {
            max_open_local_unidirectional_streams: (self.app_max_local_uni as u64)
                .try_into()
                .unwrap(),
            max_open_local_bidirectional_streams: (self.app_max_local_bidi as u64)
                .try_into()
                .unwrap(),
            ..Default::default()
        };

        // OpenRemoteBidi (initial_local_limits)
        initial_local_limits.max_open_remote_bidirectional_streams =
            VarInt::from_u32(self.initial_local_max_remote_bidi.into());

        // OpenRemoteUni (initial_local_limits)
        initial_local_limits.max_open_remote_unidirectional_streams =
            VarInt::from_u32(self.initial_local_max_remote_uni.into());

        // OpenLocalBidi (initial_remote_limits)
        //  initial_remote_max_local_bidi.min(app_max_local_bidi)
        initial_remote_limits.max_open_remote_bidirectional_streams =
            VarInt::from_u32(self.initial_remote_max_local_bidi.into());

        // OpenLocalUni (initial_remote_limits)
        //  initial_remote_max_local_uni.min(app_max_local_uni)
        initial_remote_limits.max_open_remote_unidirectional_streams =
            VarInt::from_u32(self.initial_remote_max_local_uni.into());

        (initial_local_limits, initial_remote_limits, stream_limits)
    }
}

#[test]
fn model_test() {
    check!()
        .with_type::<(Limits, Vec<Operation>)>()
        .for_each(|(limits, operations)| {
            let local_endpoint_type = endpoint::Type::Server;

            let mut model = Model::new(local_endpoint_type, *limits);
            let mut clock = Clock::default();
            for operation in operations.iter() {
                model.apply(operation);
                model.subject.on_timeout(clock.get_time());
                clock.inc_by(DEFAULT_INITIAL_RTT);
            }

            model.invariants();
        })
}
