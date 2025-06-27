// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event,
    packet::number::PacketNumberSpace,
    path,
    path::MINIMUM_MAX_DATAGRAM_SIZE,
    recovery::{
        congestion_controller::PathPublisher,
        pacing::{Pacer, INITIAL_INTERVAL, N, SLOW_START_N},
        RttEstimator, MAX_BURST_PACKETS,
    },
    time::{Clock, NoopClock, Timestamp},
};
use bolero::{check, generator::*};
use core::time::Duration;
use num_traits::ToPrimitive;

#[test]
fn earliest_departure_time() {
    let mut pacer = Pacer::default();
    let mut publisher = event::testing::Publisher::snapshot();
    let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());
    assert_eq!(None, pacer.next_packet_departure_time);

    let now = NoopClock.get_time();
    let rtt = RttEstimator::default();
    let cwnd = 12000;

    pacer.on_packet_sent(
        now,
        MINIMUM_MAX_DATAGRAM_SIZE as usize,
        &rtt,
        cwnd,
        MINIMUM_MAX_DATAGRAM_SIZE,
        false,
        &mut publisher,
    );

    // The initial interval is added for the second packet
    assert_eq!(
        Some(now + INITIAL_INTERVAL),
        pacer.earliest_departure_time()
    );
}

#[test]
fn on_packet_sent_large_bytes_sent() {
    let mut pacer = Pacer::default();
    let mut publisher = event::testing::Publisher::snapshot();
    let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());

    let now = NoopClock.get_time();
    let rtt = RttEstimator::default();
    let cwnd = 12000;

    pacer.on_packet_sent(
        now,
        usize::MAX,
        &rtt,
        cwnd,
        MINIMUM_MAX_DATAGRAM_SIZE,
        false,
        &mut publisher,
    );

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
    let rtt = RttEstimator::default();
    let mut publisher = event::testing::Publisher::snapshot();
    let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());

    let cwnd = MINIMUM_MAX_DATAGRAM_SIZE as u32 * 100;
    let n = if slow_start {
        SLOW_START_N.to_f32().unwrap()
    } else {
        N.to_f32().unwrap()
    };
    // If in slow start we should be sending 2X the cwnd in one RTT,
    // otherwise we should be sending 1.25X the cwnd.
    let bytes_to_send = ((cwnd as f32) * n) as u32;

    // Send one packet to move beyond the initial interval
    pacer.on_packet_sent(
        now,
        MINIMUM_MAX_DATAGRAM_SIZE as usize,
        &rtt,
        cwnd,
        MINIMUM_MAX_DATAGRAM_SIZE,
        slow_start,
        &mut publisher,
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
            .is_none_or(|departure_time| departure_time < now + rtt.smoothed_rtt()));
        pacer.on_packet_sent(
            now,
            MINIMUM_MAX_DATAGRAM_SIZE as usize,
            &rtt,
            cwnd,
            MINIMUM_MAX_DATAGRAM_SIZE,
            slow_start,
            &mut publisher,
        );
        sent_bytes += MINIMUM_MAX_DATAGRAM_SIZE as u32;
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
    let rtt = RttEstimator::default();
    let mut publisher = event::testing::Publisher::snapshot();
    let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());

    let cwnd = MINIMUM_MAX_DATAGRAM_SIZE as u32 * 100;
    loop {
        pacer.on_packet_sent(
            now,
            MINIMUM_MAX_DATAGRAM_SIZE as usize,
            &rtt,
            cwnd,
            MINIMUM_MAX_DATAGRAM_SIZE,
            false,
            &mut publisher,
        );
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
    pacer.on_packet_sent(
        now,
        MINIMUM_MAX_DATAGRAM_SIZE as usize,
        &rtt,
        cwnd,
        MINIMUM_MAX_DATAGRAM_SIZE,
        false,
        &mut publisher,
    );
    assert_eq!(Some(now), pacer.earliest_departure_time());
}

#[test]
fn interval_change() {
    let mut pacer = Pacer::default();
    let now = NoopClock.get_time();
    let mut rtt = RttEstimator::default();

    let mut cwnd = MINIMUM_MAX_DATAGRAM_SIZE as u32 * 100;

    if INITIAL_INTERVAL > Duration::ZERO {
        let interval = get_interval(
            now,
            &mut pacer,
            &rtt,
            cwnd,
            MINIMUM_MAX_DATAGRAM_SIZE,
            false,
        );
        assert_eq!(INITIAL_INTERVAL, interval);
    }

    let interval = get_interval(
        now,
        &mut pacer,
        &rtt,
        cwnd,
        MINIMUM_MAX_DATAGRAM_SIZE,
        false,
    );

    cwnd += MINIMUM_MAX_DATAGRAM_SIZE as u32;
    let new_interval = get_interval(
        now,
        &mut pacer,
        &rtt,
        cwnd,
        MINIMUM_MAX_DATAGRAM_SIZE,
        false,
    );

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
    let new_interval = get_interval(
        now,
        &mut pacer,
        &rtt,
        cwnd,
        MINIMUM_MAX_DATAGRAM_SIZE,
        false,
    );

    // Interval increases after the RTT increases, as the same amount of data is distributed over
    // a longer time period
    assert!(new_interval > interval);

    let interval = new_interval;
    let max_datagram_size = MINIMUM_MAX_DATAGRAM_SIZE + 300;
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

/// This test aims to compare the rate based implementation of pacing with the inter-packet interval
/// based implementation of pacing described in RFC 9002. Due to rounding issues while multiplying
/// and dividing, the two implementations do not match exactly, so this test asserts that the
/// two implementations differ by less than 1.1 ms.
#[test]
#[cfg_attr(miri, ignore)] // This test is too expensive for miri to complete in a reasonable amount of time
fn interval_differential_test() {
    check!()
        .with_generator((
            1_000_000..u32::MAX,              // RTT ranges from 1ms to ~4sec
            2400..u32::MAX, // congestion window ranges from the minimum window (2 * MINIMUM_MAX_DATAGRAM_SIZE) to u32::MAX
            MINIMUM_MAX_DATAGRAM_SIZE..=9000, // max_datagram_size ranges from MINIMUM_MAX_DATAGRAM_SIZE to 9000
            produce(),
        ))
        .cloned()
        .for_each(|(rtt, congestion_window, max_datagram_size, slow_start)| {
            let mut publisher = event::testing::Publisher::no_snapshot();
            let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());
            let rtt = Duration::from_nanos(rtt as _);
            let actual = Pacer::interval(
                rtt,
                congestion_window,
                max_datagram_size,
                slow_start,
                &mut publisher,
            );

            let expected = rfc_interval(rtt, congestion_window, max_datagram_size, slow_start);

            assert!(
                abs_difference(actual, expected) < Duration::from_nanos(1_100_000),
                "expected: {expected:?}; actual: {actual:?}"
            );
        });
}

//= https://www.rfc-editor.org/rfc/rfc9002#section-7.7
//= type=test
//# A perfectly paced sender spreads packets exactly evenly over time.
//# For a window-based congestion controller, such as the one in this
//# document, that rate can be computed by averaging the congestion
//# window over the RTT. Expressed as a rate in units of bytes per time,
//# where congestion_window is in bytes:
//#
//# rate = N * congestion_window / smoothed_rtt
//#
//# Or expressed as an inter-packet interval in units of time:
//#
//# interval = ( smoothed_rtt * packet_size / congestion_window ) / N
//(rtt_estimator.smoothed_rtt() * packet_size / congestion_window) / n
fn rfc_interval(
    rtt: Duration,
    congestion_window: u32,
    max_datagram_size: u16,
    slow_start: bool,
) -> Duration {
    let packet_size = MAX_BURST_PACKETS * max_datagram_size as u32;
    let result = rtt * packet_size / congestion_window;
    let n = if slow_start { SLOW_START_N } else { N };

    // Divide by n by multiplying by the inverse
    result * *n.denom() as u32 / *n.numer() as u32
}

fn abs_difference(a: Duration, b: Duration) -> Duration {
    a.abs_diff(b)
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
    let mut publisher = event::testing::Publisher::no_snapshot();
    let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());
    let starting_departure_time = pacer.earliest_departure_time().unwrap_or(now);

    loop {
        pacer.on_packet_sent(
            now,
            max_datagram_size as usize,
            rtt_estimator,
            congestion_window,
            max_datagram_size,
            slow_start,
            &mut publisher,
        );
        if let Some(departure_time) = pacer.earliest_departure_time() {
            if departure_time > starting_departure_time {
                return departure_time - starting_departure_time;
            }
        }
    }
}
