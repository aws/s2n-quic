// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{generator::gen_duration, Network, NetworkEvent, Report};
use alloc::collections::VecDeque;
use bolero::generator::*;
use s2n_quic_core::time::{clock::testing as time, Duration};

#[derive(Clone, Debug, TypeGenerator)]
pub struct Simulation {
    pub network: Network,
    pub events: VecDeque<NetworkEvent>,
    #[generator(gen_duration())]
    pub delay: Duration,
}

impl Simulation {
    pub fn run(&mut self) -> Report {
        let mut events = self.events.iter().cloned().cycle();
        let delay = self.delay;
        let mut report = Report::default();

        self.network.init(time::now());

        while let Some(now) = self.network.next_tick() {
            self.network.tick(now, &mut events, delay, &mut report);
            report.iterations += 1;
        }

        self.network.done();

        report.client.pending_ack_ranges = self
            .network
            .client
            .application
            .endpoint
            .ack_manager
            .ack_ranges
            .clone();
        report.server.pending_ack_ranges = self
            .network
            .server
            .application
            .endpoint
            .ack_manager
            .ack_ranges
            .clone();

        report
    }
}
