// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::contexts::testing::{MockWriteContext, OutgoingFrameBuffer};
use s2n_quic_core::{
    endpoint,
    time::{clock::testing as time, Timestamp},
    transmission,
};

#[derive(Clone, Debug)]
pub struct TestEnvironment {
    pub sent_frames: OutgoingFrameBuffer,
    pub current_time: Timestamp,
    pub transmission_constraint: transmission::Constraint,
    pub transmission_mode: transmission::Mode,
    pub local_endpoint_type: endpoint::Type,
}

impl Default for TestEnvironment {
    fn default() -> Self {
        Self::new()
    }
}

impl TestEnvironment {
    pub fn new() -> Self {
        let mut sent_frames = OutgoingFrameBuffer::new();

        sent_frames.set_max_packet_size(Some(1200));

        Self {
            sent_frames,
            current_time: time::now(),
            transmission_constraint: transmission::Constraint::None,
            transmission_mode: transmission::Mode::Normal,
            local_endpoint_type: endpoint::Type::Server,
        }
    }

    pub fn context(&mut self) -> MockWriteContext<'_> {
        MockWriteContext::new(
            self.current_time,
            &mut self.sent_frames,
            self.transmission_constraint,
            self.transmission_mode,
            self.local_endpoint_type,
        )
    }
}
