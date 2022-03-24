// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::time::{Clock, NoopClock};

#[test]
fn on_packet_sent_timestamp_initialization() {
    let mut bw_estimator = BandwidthEstimator::default();

    let now = NoopClock.get_time();

    // Test that first_sent_time and delivered_time are updated if they are None
    assert_eq!(None, bw_estimator.first_sent_time());
    assert_eq!(None, bw_estimator.delivered_time());
    bw_estimator.on_packet_sent(false, false, now);
    assert_eq!(Some(now), bw_estimator.first_sent_time());
    assert_eq!(Some(now), bw_estimator.delivered_time());

    let new_now = now + Duration::from_secs(5);

    // Test that first_sent_time and delivered_time are not updated if packets are in flight
    bw_estimator.on_packet_sent(true, false, new_now);
    assert_eq!(Some(now), bw_estimator.first_sent_time());
    assert_eq!(Some(now), bw_estimator.delivered_time());

    // Test that first_sent_time and delivered_time are updated if packets are not in flight
    bw_estimator.on_packet_sent(false, false, new_now);
    assert_eq!(Some(new_now), bw_estimator.first_sent_time());
    assert_eq!(Some(new_now), bw_estimator.delivered_time());
}

#[test]
fn on_packet_sent_app_limited() {
    let mut bw_estimator = BandwidthEstimator::default();

    let now = NoopClock.get_time();

    bw_estimator.on_packet_sent(false, false, now);
    assert_eq!(None, bw_estimator.app_limited_timestamp());

    bw_estimator.on_packet_sent(false, true, now);
    assert_eq!(Some(now), bw_estimator.app_limited_timestamp());
}

#[test]
fn on_packet_loss() {
    let mut bw_estimator = BandwidthEstimator::default();

    assert_eq!(0, bw_estimator.lost_bytes());
    assert_eq!(0, bw_estimator.rate_sample.newly_lost);
    assert_eq!(0, bw_estimator.rate_sample.lost);

    bw_estimator.on_packet_loss(500);

    assert_eq!(500, bw_estimator.lost_bytes());
    assert_eq!(500, bw_estimator.rate_sample.newly_lost);
    assert_eq!(500, bw_estimator.rate_sample.lost);

    bw_estimator.on_packet_loss(250);

    assert_eq!(750, bw_estimator.lost_bytes());
    assert_eq!(750, bw_estimator.rate_sample.newly_lost);
    assert_eq!(750, bw_estimator.rate_sample.lost);

    // Simulate a new ACK arriving, this would reset newly_lost and set prior_lost
    bw_estimator.rate_sample.newly_lost = 0;
    bw_estimator.rate_sample.prior_lost = 750;

    bw_estimator.on_packet_loss(250);

    assert_eq!(1000, bw_estimator.lost_bytes());
    assert_eq!(250, bw_estimator.rate_sample.newly_lost);
    assert_eq!(250, bw_estimator.rate_sample.lost);
}
