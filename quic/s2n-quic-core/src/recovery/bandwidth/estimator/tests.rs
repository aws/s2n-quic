// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::{
    event, path,
    recovery::congestion_controller::PathPublisher,
    time::{Clock, NoopClock},
};

#[test]
fn bandwidth() {
    let result = Bandwidth::new(1000, Duration::from_secs(10));

    // 10 seconds = 10^10 nanoseconds, nanos_per_byte = 10^10 / 1000 = 10_000_000
    // nanos_per_kibibyte = 10_000_000 * 1024 =
    assert_eq!(10_240_000_000, result.nanos_per_kibibyte);
}

#[test]
fn bandwidth_saturating() {
    let result = Bandwidth::new(u64::MAX, Duration::from_secs(1));

    assert_eq!(Bandwidth::INFINITY, result);
}

#[test]
fn bandwidth_zero_interval() {
    let result = Bandwidth::new(500, Duration::ZERO);

    assert_eq!(Bandwidth::ZERO, result);
}
/// Test that the maximum supported interval (~104 days) does not panic
#[test]
fn bandwidth_max_interval() {
    let result = Bandwidth::new(1, Duration::from_nanos(1 << 53));

    assert!(result > Bandwidth::ZERO);
}

/// Test that a value larger than the maximum supported interval does not panic
#[test]
fn bandwidth_interval_too_long() {
    let result = Bandwidth::new(1, Duration::from_nanos(1 << 54));

    assert_eq!(Bandwidth::ZERO, result);
}

#[test]
fn bandwidth_mul_ratio() {
    let bandwidth = Bandwidth::new(4000, Duration::from_secs(1));

    let result = bandwidth * Ratio::new(3, 4);

    assert_eq!(result, Bandwidth::new(3000, Duration::from_secs(1)));
}

#[test]
fn bandwidth_zero_mul_ratio() {
    assert_eq!(Bandwidth::ZERO, Bandwidth::ZERO * Ratio::new(3, 7));
}

#[test]
fn bandwidth_mul_duration() {
    let bandwidth = Bandwidth::new(7000, Duration::from_secs(2));

    let result = bandwidth * Duration::from_secs(10);

    assert_eq!(result, 35000);
}

#[test]
fn bandwidth_mul_duration_too_long() {
    let bandwidth = Bandwidth::new(1, Duration::from_nanos(1));

    let result = bandwidth * Duration::from_nanos(1 << 53);

    assert!(result < u64::MAX);

    let result = bandwidth * Duration::from_nanos(1 << 54);

    assert_eq!(result, u64::MAX);
}

#[test]
fn bandwidth_mul_saturation() {
    let bandwidth = Bandwidth::INFINITY;

    let result = bandwidth * Duration::from_secs(10);

    assert_eq!(result, u64::MAX);
}

#[test]
fn u64_div_bandwidth() {
    let bandwidth = Bandwidth::new(10_000, Duration::from_secs(1));
    let bytes = 200_000;
    assert_eq!(bytes / bandwidth, Duration::from_secs(20));

    let bandwidth = Bandwidth::new(10_000, Duration::from_secs(1));
    let bytes = 2_000;
    assert_eq!(bytes / bandwidth, Duration::from_millis(200));
}

#[test]
fn bandwidth_ordering() {
    let low = Bandwidth::new(10_000, Duration::from_secs(1));
    let high = Bandwidth::new(20_000, Duration::from_secs(1));

    assert!(high > low);
    assert_eq!(high, low.max(high));
}

// first_sent_time and delivered_time typically hold values from recently acknowledged packets. However,
// when  no packet has been sent yet, or there are no packets currently in flight, these values are initialized
// with the time when a packet is sent. This test confirms first_sent_time and delivered_time are
// initialized properly on the first packet sent, and on the first packet sent after an idle period.
#[test]
fn on_packet_sent_timestamp_initialization() {
    let t0 = NoopClock.get_time();
    let mut bw_estimator = Estimator::default();

    // Test that first_sent_time and delivered_time are updated on the first sent packet
    let packet_info = bw_estimator.on_packet_sent(0, 0, None, t0);
    assert_eq!(t0, packet_info.first_sent_time);
    assert_eq!(t0, packet_info.delivered_time);
    assert_eq!(Some(t0), bw_estimator.first_sent_time);
    assert_eq!(Some(t0), bw_estimator.delivered_time);

    // Test that first_sent_time and delivered_time are not updated if packets are in flight
    let t1 = t0 + Duration::from_secs(1);
    let packet_info = bw_estimator.on_packet_sent(1500, 0, None, t1);
    assert_eq!(t0, packet_info.first_sent_time);
    assert_eq!(t0, packet_info.delivered_time);
    assert_eq!(Some(t0), bw_estimator.first_sent_time);
    assert_eq!(Some(t0), bw_estimator.delivered_time);

    // Test that first_sent_time and delivered_time are updated after an idle period
    let t2 = t0 + Duration::from_secs(2);
    let packet_info = bw_estimator.on_packet_sent(0, 0, None, t2);
    assert_eq!(t2, packet_info.first_sent_time);
    assert_eq!(t2, packet_info.delivered_time);
    assert_eq!(Some(t2), bw_estimator.first_sent_time);
    assert_eq!(Some(t2), bw_estimator.delivered_time);
}

#[test]
fn on_packet_sent() {
    let first_sent_time = NoopClock.get_time();
    let delivered_time = first_sent_time + Duration::from_secs(1);
    let mut bw_estimator = Estimator {
        delivered_bytes: 15000,
        delivered_time: Some(delivered_time),
        lost_bytes: 100,
        ecn_ce_count: 5,
        first_sent_time: Some(first_sent_time),
        app_limited_delivered_bytes: None,
        rate_sample: Default::default(),
    };

    let packet_info = bw_estimator.on_packet_sent(500, 100, Some(true), first_sent_time);
    assert_eq!(first_sent_time, packet_info.first_sent_time);
    assert_eq!(delivered_time, packet_info.delivered_time);
    assert_eq!(15000, packet_info.delivered_bytes);
    assert_eq!(100, packet_info.lost_bytes);
    assert_eq!(5, packet_info.ecn_ce_count);
    assert!(packet_info.is_app_limited);
    assert_eq!(500 + 100, packet_info.bytes_in_flight);
    assert_eq!(
        Some(500 + 15000 + 100),
        bw_estimator.app_limited_delivered_bytes
    );
}

#[test]
fn app_limited() {
    let mut publisher = event::testing::Publisher::snapshot();
    let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());
    let first_sent_time = NoopClock.get_time();
    let delivered_time = first_sent_time + Duration::from_secs(1);
    let mut bw_estimator = Estimator {
        delivered_bytes: 15000,
        delivered_time: Some(delivered_time),
        lost_bytes: 100,
        ecn_ce_count: 5,
        first_sent_time: Some(first_sent_time),
        app_limited_delivered_bytes: None,
        rate_sample: Default::default(),
    };

    // Packet is sent while app-limited, starting the app limited period
    let packet_info = bw_estimator.on_packet_sent(1500, 100, Some(true), first_sent_time);
    assert!(packet_info.is_app_limited);
    assert_eq!(
        Some(1500 + 15000 + 100),
        bw_estimator.app_limited_delivered_bytes
    );

    // Packet is sent while not app-limited, but the app limited continues until all previous bytes in flight have been acknowledged
    let packet_info = bw_estimator.on_packet_sent(500, 100, Some(false), first_sent_time);
    assert!(packet_info.is_app_limited);
    assert_eq!(
        Some(1500 + 15000 + 100),
        bw_estimator.app_limited_delivered_bytes
    );

    // Packet is sent while app-limited is not determined, this should default to app-limited
    let packet_info = bw_estimator.on_packet_sent(2500, 100, None, first_sent_time);
    assert!(packet_info.is_app_limited);
    assert_eq!(
        Some(2500 + 15000 + 100),
        bw_estimator.app_limited_delivered_bytes
    );

    let packet_info = PacketInfo {
        delivered_bytes: bw_estimator.delivered_bytes,
        delivered_time,
        lost_bytes: 0,
        ecn_ce_count: 0,
        first_sent_time,
        bytes_in_flight: 1500,
        is_app_limited: false,
    };

    // Acknowledge all the bytes that were inflight when the app-limited period began
    bw_estimator.on_ack(
        2600,
        delivered_time,
        packet_info,
        delivered_time,
        &mut publisher,
    );
    // Still app_limited, since we need bytes to be acknowledged after the app limited period
    assert_eq!(
        Some(2500 + 100 + 15000),
        bw_estimator.app_limited_delivered_bytes
    );

    // Acknowledge one more byte
    bw_estimator.on_ack(
        1,
        delivered_time,
        packet_info,
        delivered_time,
        &mut publisher,
    );
    // Now the app limited period is over
    assert_eq!(None, bw_estimator.app_limited_delivered_bytes);
}

#[test]
fn on_packet_ack_rate_sample() {
    let mut publisher = event::testing::Publisher::snapshot();
    let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());
    let t0 = NoopClock.get_time() + Duration::from_secs(60);
    let t1 = t0 + Duration::from_secs(1);
    let t2 = t0 + Duration::from_secs(2);
    let mut bw_estimator = Estimator::default();

    // Send three packets. In between each send, other packets were acknowledged, lost or had ECN CE,
    // and thus the delivered_bytes amount, lost_bytes amount, and ecn ce count is increased.
    let packet_1 = bw_estimator.on_packet_sent(0, 100, Some(false), t0);
    bw_estimator.delivered_bytes = 100000;
    bw_estimator.lost_bytes = 100;
    bw_estimator.ecn_ce_count = 5;
    let packet_2 = bw_estimator.on_packet_sent(1500, 100, Some(true), t1);
    bw_estimator.delivered_bytes = 200000;
    bw_estimator.lost_bytes = 150;
    bw_estimator.ecn_ce_count = 15;
    let packet_3 = bw_estimator.on_packet_sent(3000, 100, Some(false), t2);

    let now = t0 + Duration::from_secs(10);
    let delivered_bytes = bw_estimator.delivered_bytes;
    bw_estimator.on_ack(1500, t0, packet_1, now, &mut publisher);

    assert_eq!(bw_estimator.delivered_bytes, delivered_bytes + 1500);
    assert_eq!(bw_estimator.delivered_time, Some(now));

    // Rate sample is updated since this is the first packet delivered
    assert_eq!(
        packet_1.delivered_bytes,
        bw_estimator.rate_sample.prior_delivered_bytes
    );
    assert_eq!(
        packet_1.lost_bytes,
        bw_estimator.rate_sample.prior_lost_bytes
    );
    assert_eq!(
        packet_1.is_app_limited,
        bw_estimator.rate_sample.is_app_limited
    );
    assert_eq!(
        packet_1.bytes_in_flight,
        bw_estimator.rate_sample.bytes_in_flight
    );
    assert_eq!(Some(t0), bw_estimator.first_sent_time);
    assert_eq!(now - t0, bw_estimator.rate_sample.interval);

    // Delivered bytes, lost bytes, and ECN CE counts reflect the total lifetime counts,
    // since there was no prior delivered packets
    assert_eq!(
        bw_estimator.delivered_bytes,
        bw_estimator.rate_sample.delivered_bytes
    );
    assert_eq!(bw_estimator.lost_bytes, bw_estimator.rate_sample.lost_bytes);
    assert_eq!(
        bw_estimator.ecn_ce_count,
        bw_estimator.rate_sample.ecn_ce_count
    );

    // Ack a newer packet
    let now = now + Duration::from_secs(1);
    let delivered_bytes = bw_estimator.delivered_bytes;
    bw_estimator.on_ack(1500, t2, packet_3, now, &mut publisher);

    assert_eq!(bw_estimator.delivered_bytes, delivered_bytes + 1500);
    assert_eq!(bw_estimator.delivered_time, Some(now));

    // Rate sample is updated since this packet is newer (has a higher delivered_bytes)
    assert!(packet_3.delivered_bytes > packet_1.delivered_bytes);
    assert_eq!(
        packet_3.delivered_bytes,
        bw_estimator.rate_sample.prior_delivered_bytes
    );
    assert_eq!(
        packet_3.lost_bytes,
        bw_estimator.rate_sample.prior_lost_bytes
    );
    assert_eq!(
        packet_3.is_app_limited,
        bw_estimator.rate_sample.is_app_limited
    );
    assert_eq!(
        packet_3.bytes_in_flight,
        bw_estimator.rate_sample.bytes_in_flight
    );
    assert_eq!(Some(t2), bw_estimator.first_sent_time);
    assert_eq!(now - t0, bw_estimator.rate_sample.interval);

    // Now the delivered_bytes in the rate sample should reflect the current lifetime delivered bytes (200000 + 1500 + 1500)
    // minus the delivered bytes value at the time this packet was transmitted (200000)
    assert_eq!(3000, bw_estimator.rate_sample.delivered_bytes);
    // Lost bytes and ECN CE count are 0 since no additional loss or ECN CE was received since this packet was transmitted
    assert_eq!(0, bw_estimator.rate_sample.lost_bytes);
    assert_eq!(0, bw_estimator.rate_sample.ecn_ce_count);

    // Ack an older packet
    let now = now + Duration::from_secs(1);
    let delivered_bytes = bw_estimator.delivered_bytes;
    bw_estimator.on_ack(1500, t1, packet_2, now, &mut publisher);

    assert_eq!(bw_estimator.delivered_bytes, delivered_bytes + 1500);
    assert_eq!(bw_estimator.delivered_time, Some(now));

    //= https://tools.ietf.org/id/draft-cheng-iccrg-delivery-rate-estimation-02#3.3
    //= type=test
    //# UpdateRateSample() is invoked multiple times when a stretched ACK acknowledges
    //# multiple data packets. In this case we use the information from the most recently
    //# sent packet, i.e., the packet with the highest "P.delivered" value.

    // Rate sample is not updated since this packet is older than the current sample
    assert_eq!(
        packet_3.delivered_bytes,
        bw_estimator.rate_sample.prior_delivered_bytes
    );
    assert_eq!(
        packet_3.lost_bytes,
        bw_estimator.rate_sample.prior_lost_bytes
    );
    assert_eq!(
        packet_3.is_app_limited,
        bw_estimator.rate_sample.is_app_limited
    );
    assert_eq!(
        packet_3.bytes_in_flight,
        bw_estimator.rate_sample.bytes_in_flight
    );
    assert_eq!(Some(t2), bw_estimator.first_sent_time);

    // Delivered bytes is increased to include packet 2, as it is within the sampling interval
    assert_eq!(4500, bw_estimator.rate_sample.delivered_bytes);
    assert_eq!(0, bw_estimator.rate_sample.lost_bytes);
    assert_eq!(0, bw_estimator.rate_sample.ecn_ce_count);
}

//= https://tools.ietf.org/id/draft-cheng-iccrg-delivery-rate-estimation-02#2.2.4
//= type=test
//# Since it is physically impossible to have data delivered faster than it is sent
//# in a sustained fashion, when the estimator notices that the ack_rate for a flight
//# is faster than the send rate for the flight, it filters out the implausible ack_rate
//# by capping the delivery rate sample to be no higher than the send rate.
#[test]
fn on_packet_ack_implausible_ack_rate() {
    let mut publisher = event::testing::Publisher::snapshot();
    let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());
    let t0 = NoopClock.get_time();
    let mut bw_estimator = Estimator::default();

    // A packet is sent and acknowledged 4 seconds later
    let packet_info = bw_estimator.on_packet_sent(0, 100, Some(false), t0);
    let t4 = t0 + Duration::from_secs(4);
    bw_estimator.on_ack(1500, t0, packet_info, t4, &mut publisher);

    // A packet is sent and acknowledged 1 second later
    let t5 = t0 + Duration::from_secs(5);
    let packet_info = bw_estimator.on_packet_sent(1500, 100, Some(false), t5);
    let now = t0 + Duration::from_secs(6);
    bw_estimator.on_ack(
        1500,
        t0 + Duration::from_secs(5),
        packet_info,
        now,
        &mut publisher,
    );

    let send_elapsed = t5 - packet_info.first_sent_time;
    let ack_elapsed = now - packet_info.delivered_time;

    // The amount of time to send the packets was greater than the time to
    // acknowledge them, indicating the ack rate would be implausible.
    assert!(send_elapsed > ack_elapsed);

    // The rate sample interval is based on the send_elapsed time since the
    // ack_elapsed time was implausible
    assert_eq!(send_elapsed, bw_estimator.rate_sample.interval);
}

#[test]
fn on_packet_loss() {
    let mut bw_estimator = Estimator::default();

    assert_eq!(0, bw_estimator.lost_bytes);
    assert_eq!(0, bw_estimator.rate_sample.lost_bytes);

    bw_estimator.on_loss(500);

    assert_eq!(500, bw_estimator.lost_bytes);
    assert_eq!(500, bw_estimator.rate_sample.lost_bytes);

    bw_estimator.on_loss(250);

    assert_eq!(750, bw_estimator.lost_bytes);
    assert_eq!(750, bw_estimator.rate_sample.lost_bytes);

    bw_estimator.on_app_limited(1000);
    bw_estimator.on_loss(250);
    assert_eq!(Some(750), bw_estimator.app_limited_delivered_bytes);

    bw_estimator.on_loss(1000);
    assert_eq!(Some(0), bw_estimator.app_limited_delivered_bytes);
}

#[test]
fn on_packet_discarded() {
    let mut bw_estimator = Estimator::default();

    bw_estimator.on_app_limited(1000);
    bw_estimator.on_packet_discarded(250);
    assert_eq!(Some(750), bw_estimator.app_limited_delivered_bytes);

    bw_estimator.on_packet_discarded(1000);
    assert_eq!(Some(0), bw_estimator.app_limited_delivered_bytes);
}

#[test]
fn on_explicit_congestion() {
    let mut bw_estimator = Estimator::default();

    assert_eq!(0, bw_estimator.ecn_ce_count);
    assert_eq!(0, bw_estimator.rate_sample.ecn_ce_count);

    bw_estimator.on_explicit_congestion(5);

    assert_eq!(5, bw_estimator.ecn_ce_count);
    assert_eq!(5, bw_estimator.rate_sample.ecn_ce_count);

    bw_estimator.on_explicit_congestion(3);

    assert_eq!(8, bw_estimator.ecn_ce_count);
    assert_eq!(8, bw_estimator.rate_sample.ecn_ce_count);
}

#[test]
fn as_bytes_per_second() {
    let bandwidth = Bandwidth::new(10_000, Duration::from_secs(1));

    assert_eq!(10_000, bandwidth.as_bytes_per_second());
    assert_eq!(0, Bandwidth::ZERO.as_bytes_per_second());
    assert_eq!(u64::MAX, Bandwidth::INFINITY.as_bytes_per_second());
}
