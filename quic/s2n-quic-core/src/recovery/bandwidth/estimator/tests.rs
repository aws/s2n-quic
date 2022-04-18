// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::time::{Clock, NoopClock};

// first_sent_time and delivered_time typically hold values from recently acknowledged packets. However,
// when  no packet has been sent yet, or there are no packets currently in flight, these values are initialized
// with the time when a packet is sent. This test confirms first_sent_time and delivered_time are
// initialized properly on the first packet sent, and on the first packet sent after an idle period.
#[test]
fn on_packet_sent_timestamp_initialization() {
    let t0 = NoopClock.get_time();
    let mut bw_estimator = Estimator::default();

    // Test that first_sent_time and delivered_time are updated on the first sent packet
    let packet_info = bw_estimator.on_packet_sent(0, false, t0);
    assert_eq!(t0, packet_info.first_sent_time);
    assert_eq!(t0, packet_info.delivered_time);
    assert_eq!(Some(t0), bw_estimator.first_sent_time);
    assert_eq!(Some(t0), bw_estimator.delivered_time);

    // Test that first_sent_time and delivered_time are not updated if packets are in flight
    let t1 = t0 + Duration::from_secs(1);
    let packet_info = bw_estimator.on_packet_sent(1500, false, t1);
    assert_eq!(t0, packet_info.first_sent_time);
    assert_eq!(t0, packet_info.delivered_time);
    assert_eq!(Some(t0), bw_estimator.first_sent_time);
    assert_eq!(Some(t0), bw_estimator.delivered_time);

    // Test that first_sent_time and delivered_time are updated after an idle period
    let t2 = t0 + Duration::from_secs(2);
    let packet_info = bw_estimator.on_packet_sent(0, false, t2);
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
        first_sent_time: Some(first_sent_time),
        rate_sample: Default::default(),
    };

    let packet_info = bw_estimator.on_packet_sent(500, true, first_sent_time);
    assert_eq!(first_sent_time, packet_info.first_sent_time);
    assert_eq!(delivered_time, packet_info.delivered_time);
    assert_eq!(15000, packet_info.delivered_bytes);
    assert_eq!(100, packet_info.lost_bytes);
    assert!(packet_info.is_app_limited);
    assert_eq!(500, packet_info.bytes_in_flight);
}

#[test]
fn on_packet_ack_rate_sample() {
    let t0 = NoopClock.get_time() + Duration::from_secs(60);
    let t1 = t0 + Duration::from_secs(1);
    let t2 = t0 + Duration::from_secs(2);
    let mut bw_estimator = Estimator::default();

    // Send three packets. In between each send, other packets were acknowledged, and thus the
    // delivered_bytes amount is increased.
    let packet_1 = bw_estimator.on_packet_sent(0, false, t0);
    bw_estimator.delivered_bytes = 100000;
    let packet_2 = bw_estimator.on_packet_sent(1500, true, t1);
    bw_estimator.delivered_bytes = 200000;
    let packet_3 = bw_estimator.on_packet_sent(3000, false, t2);

    let now = t0 + Duration::from_secs(10);
    let delivered_bytes = bw_estimator.delivered_bytes;
    bw_estimator.on_ack(1500, t0, packet_1, now);

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

    // Ack a newer packet
    let now = now + Duration::from_secs(1);
    let delivered_bytes = bw_estimator.delivered_bytes;
    bw_estimator.on_ack(1500, t2, packet_3, now);

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

    // Ack an older packet
    let now = now + Duration::from_secs(1);
    let delivered_bytes = bw_estimator.delivered_bytes;
    bw_estimator.on_ack(1500, t1, packet_2, now);

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
}

//= https://tools.ietf.org/id/draft-cheng-iccrg-delivery-rate-estimation-02#2.2.4
//= type=test
//# Since it is physically impossible to have data delivered faster than it is sent
//# in a sustained fashion, when the estimator notices that the ack_rate for a flight
//# is faster than the send rate for the flight, it filters out the implausible ack_rate
//# by capping the delivery rate sample to be no higher than the send rate.
#[test]
fn on_packet_ack_implausible_ack_rate() {
    let t0 = NoopClock.get_time();
    let mut bw_estimator = Estimator::default();

    // A packet is sent and acknowledged 4 seconds later
    let packet_info = bw_estimator.on_packet_sent(0, false, t0);
    let t4 = t0 + Duration::from_secs(4);
    bw_estimator.on_ack(1500, t0, packet_info, t4);

    // A packet is sent and acknowledged 1 second later
    let t5 = t0 + Duration::from_secs(5);
    let packet_info = bw_estimator.on_packet_sent(1500, false, t5);
    let now = t0 + Duration::from_secs(6);
    bw_estimator.on_ack(1500, t0 + Duration::from_secs(5), packet_info, now);

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
}
