// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{generator::gen_duration, Endpoint, Packet};
use alloc::collections::VecDeque;
use bolero::generator::*;
use core::time::Duration;
use s2n_quic_core::{endpoint, time::Timestamp};

#[derive(Clone, Debug, TypeGenerator)]
pub struct Application {
    pub transmissions: VecDeque<ApplicationTransmission>,
    #[generator(constant(None))]
    next_transmission: Option<Timestamp>,
    pub endpoint: Endpoint,
}

impl Application {
    pub fn new<I: Iterator<Item = Duration>>(endpoint: Endpoint, transmissions: I) -> Self {
        Self {
            transmissions: transmissions.map(Into::into).collect(),
            next_transmission: None,
            endpoint,
        }
    }

    pub fn init(&mut self, now: Timestamp, endpoint_type: endpoint::Type) {
        self.init_next_transmission(now);
        self.endpoint.init(now, endpoint_type);
    }

    fn init_next_transmission(&mut self, now: Timestamp) {
        self.next_transmission = self.transmissions.pop_front().map(|t| now + t.delay);
    }

    pub fn recv(&mut self, packet: Packet) {
        self.endpoint.recv(packet)
    }

    pub fn tick(&mut self, now: Timestamp) -> Option<Packet> {
        if self.next_transmission.is_none() {
            self.init_next_transmission(now);
        }

        if self.next_transmission == Some(now) {
            self.init_next_transmission(now);
            self.endpoint.send(now)
        } else {
            self.endpoint.tick(now)
        }
    }

    pub fn timers(&self) -> impl Iterator<Item = &Timestamp> {
        self.endpoint.timers().chain(self.next_transmission.iter())
    }

    pub fn done(&mut self) {
        assert_eq!(
            self.transmissions.len(),
            0,
            "pending transmissions {:?}",
            self.transmissions
        );
        assert_eq!(self.next_transmission, None);
        self.endpoint.done();
    }
}

#[derive(Clone, Debug, Default, TypeGenerator)]
pub struct ApplicationTransmission {
    #[generator(gen_duration())]
    pub delay: Duration,
}

impl From<Duration> for ApplicationTransmission {
    fn from(delay: Duration) -> Self {
        Self { delay }
    }
}
