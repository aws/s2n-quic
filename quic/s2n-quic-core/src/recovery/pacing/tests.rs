// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    path::MINIMUM_MTU,
    recovery::{
        pacing::{Pacer, MAX_BURST_PACKETS, N, SLOW_START_N},
        RttEstimator,
    },
    time::{Clock, NoopClock},
};
use core::time::Duration;

#[test]
fn earliest_departure_time() {
    let mut pacer = Pacer::new(MINIMUM_MTU);
    assert_eq!(None, pacer.next_packet_departure_time);

    let now = NoopClock.get_time();
    let rtt = RttEstimator::new(Duration::default());
    let cwnd = 12000;

    pacer.on_packet_sent(now, MINIMUM_MTU as usize, &rtt, cwnd, MINIMUM_MTU, false);

    assert_eq!(Some(now), pacer.earliest_departure_time());
}

#[test]
fn slow_start() {
    let mut pacer = Pacer::new(MINIMUM_MTU);
    let now = NoopClock.get_time();
    let rtt = RttEstimator::new(Duration::default());

    let cwnd = 12000;

    // In one RTT we should be sending 2X the CWND
    let mut sent_bytes = 0;
    while sent_bytes <= (SLOW_START_N * cwnd as f32).ceil() as u32 {
        assert!(pacer
            .earliest_departure_time()
            .map_or(true, |departure_time| departure_time
                < now + rtt.smoothed_rtt()));
        pacer.on_packet_sent(now, MINIMUM_MTU as usize, &rtt, cwnd, MINIMUM_MTU, true);
        sent_bytes += MINIMUM_MTU as u32;
    }
    assert_eq!(
        Some(now + rtt.smoothed_rtt()),
        pacer.earliest_departure_time()
    );
}

#[test]
fn post_slow_start() {
    let mut pacer = Pacer::new(MINIMUM_MTU);
    let now = NoopClock.get_time();
    let rtt = RttEstimator::new(Duration::default());

    let cwnd = (MINIMUM_MTU * MAX_BURST_PACKETS) as u32 * 8;

    // In one RTT we should be sending 1.25X the CWND
    let mut sent_bytes = 0;
    while sent_bytes <= (N * cwnd as f32).ceil() as u32 {
        assert!(pacer
            .earliest_departure_time()
            .map_or(true, |departure_time| departure_time
                < now + rtt.smoothed_rtt()));
        pacer.on_packet_sent(now, MINIMUM_MTU as usize, &rtt, cwnd, MINIMUM_MTU, false);
        sent_bytes += MINIMUM_MTU as u32;
    }
    assert_eq!(
        Some(now + rtt.smoothed_rtt()),
        pacer.earliest_departure_time()
    );
}
