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
