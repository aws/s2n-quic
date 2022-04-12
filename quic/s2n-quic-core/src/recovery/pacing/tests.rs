// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    packet::number::PacketNumberSpace,
    path::MINIMUM_MTU,
    recovery::{
        pacing::{Pacer, INITIAL_INTERVAL, N, SLOW_START_N},
        RttEstimator,
    },
    time::{Clock, NoopClock, Timestamp},
};
use core::time::Duration;

#[test]
fn earliest_departure_time() {
    let mut pacer = Pacer::default();
    assert_eq!(None, pacer.next_packet_departure_time);

    let now = NoopClock.get_time();
    let rtt = RttEstimator::new(Duration::default());
    let cwnd = 12000;

    pacer.on_packet_sent(now, MINIMUM_MTU as usize, &rtt, cwnd, MINIMUM_MTU, false);

    // The initial interval is added for the second packet
    assert_eq!(
        Some(now + INITIAL_INTERVAL),
        pacer.earliest_departure_time()
    );
}

#[test]
fn on_packet_sent_large_bytes_sent() {
    let mut pacer = Pacer::default();

    let now = NoopClock.get_time();
    let rtt = RttEstimator::new(Duration::default());
    let cwnd = 12000;

    pacer.on_packet_sent(now, usize::MAX, &rtt, cwnd, MINIMUM_MTU, false);

    assert_eq!(
        Some(now + INITIAL_INTERVAL),
        pacer.earliest_departure_time()
    );
}

#[test]
fn slow_start() {
    test_one_rtt(true);
}

#[test]
fn post_slow_start() {
    test_one_rtt(false);
}

fn test_one_rtt(slow_start: bool) {
    let mut pacer = Pacer::default();
    let now = NoopClock.get_time();
    let rtt = RttEstimator::new(Duration::default());

    let cwnd = MINIMUM_MTU as u32 * 100;
    let n = if slow_start { SLOW_START_N } else { N };
    // If in slow start we should be sending 2X the cwnd in one RTT,
    // otherwise we should be sending 1.25X the cwnd.
    let bytes_to_send = cwnd * n;

    // Send one packet to move beyond the initial interval
    pacer.on_packet_sent(
        now,
        MINIMUM_MTU as usize,
        &rtt,
        cwnd,
        MINIMUM_MTU,
        slow_start,
    );
    assert_eq!(
        Some(now + INITIAL_INTERVAL),
        pacer.earliest_departure_time()
    );

    let mut sent_bytes = 0;
    let now = now + INITIAL_INTERVAL;
    while sent_bytes < bytes_to_send {
        // Confirm the current departure time is less than 1 rtt
        assert!(pacer
            .earliest_departure_time()
            .map_or(true, |departure_time| departure_time
                < now + rtt.smoothed_rtt()));
        pacer.on_packet_sent(
            now,
            MINIMUM_MTU as usize,
            &rtt,
            cwnd,
            MINIMUM_MTU,
            slow_start,
        );
        sent_bytes += MINIMUM_MTU as u32;
    }
    assert!(pacer
        .earliest_departure_time()
        .unwrap()
        .has_elapsed(now + rtt.smoothed_rtt()));
}

#[test]
fn earliest_departure_time_before_now() {
    let mut pacer = Pacer::default();
    let now = NoopClock.get_time();
    let rtt = RttEstimator::new(Duration::default());

    let cwnd = MINIMUM_MTU as u32 * 100;
    loop {
        pacer.on_packet_sent(now, MINIMUM_MTU as usize, &rtt, cwnd, MINIMUM_MTU, false);
        if pacer.capacity == 0 {
            break;
        }
    }
    assert_eq!(
        Some(now + INITIAL_INTERVAL),
        pacer.earliest_departure_time()
    );

    // The first timeslot has reached capacity, so calling `on_packet_sent` normally would move to the
    // next interval. Since the timestamp we pass in is after the next interval, the earliest departure time
    // becomes the new "now".
    let now = now + Duration::from_secs(1);
    pacer.on_packet_sent(now, MINIMUM_MTU as usize, &rtt, cwnd, MINIMUM_MTU, false);
    assert_eq!(Some(now), pacer.earliest_departure_time());
}

#[test]
fn interval_change() {
    let mut pacer = Pacer::default();
    let now = NoopClock.get_time();
    let mut rtt = RttEstimator::new(Duration::default());

    let mut cwnd = MINIMUM_MTU as u32 * 100;

    if INITIAL_INTERVAL > Duration::ZERO {
        let interval = get_interval(now, &mut pacer, &rtt, cwnd, MINIMUM_MTU, false);
        assert_eq!(INITIAL_INTERVAL, interval);
    }

    let interval = get_interval(now, &mut pacer, &rtt, cwnd, MINIMUM_MTU, false);

    cwnd += MINIMUM_MTU as u32;
    let new_interval = get_interval(now, &mut pacer, &rtt, cwnd, MINIMUM_MTU, false);

    // Interval decreases after the congestion window increases, as more bursts need to be
    // distributed evenly across the same time period (1 rtt)
    assert!(new_interval < interval);

    let interval = new_interval;
    rtt.update_rtt(
        Duration::default(),
        Duration::from_millis(750),
        now,
        true,
        PacketNumberSpace::ApplicationData,
    );
    let new_interval = get_interval(now, &mut pacer, &rtt, cwnd, MINIMUM_MTU, false);

    // Interval increases after the RTT increases, as the same amount of data is distributed over
    // a longer time period
    assert!(new_interval > interval);

    let interval = new_interval;
    let max_datagram_size = MINIMUM_MTU + 300;
    let new_interval = get_interval(now, &mut pacer, &rtt, cwnd, max_datagram_size, false);

    // Interval increases after the MTU increases, as each burst contains more data, so less
    // bursts need to be distributed across the same time period
    assert!(new_interval > interval);

    let interval = new_interval;
    let new_interval = get_interval(now, &mut pacer, &rtt, cwnd, max_datagram_size, true);

    // Interval decreases in slow start, as the rapidly increasing congestion window in slow start
    // means we want to allow for packets to be sent faster to avoid under utilizing the congestion window.
    assert!(new_interval < interval);
}

// Calls `on_packet_sent` until the earliest departure time has increased, and returns the interval
// between the new earliest departure time and the original earliest departure time
fn get_interval(
    now: Timestamp,
    pacer: &mut Pacer,
    rtt_estimator: &RttEstimator,
    congestion_window: u32,
    max_datagram_size: u16,
    slow_start: bool,
) -> Duration {
    let starting_departure_time = pacer.earliest_departure_time().unwrap_or(now);

    loop {
        pacer.on_packet_sent(
            now,
            max_datagram_size as usize,
            rtt_estimator,
            congestion_window,
            max_datagram_size,
            slow_start,
        );
        if let Some(departure_time) = pacer.earliest_departure_time() {
            if departure_time > starting_departure_time {
                return departure_time - starting_departure_time;
            }
        }
    }
}
