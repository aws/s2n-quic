// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::{
    frame::ack_elicitation::AckElicitation,
    inet::ExplicitCongestionNotification,
    path,
    time::{Clock, NoopClock},
};

#[test]
fn on_packet_sent_timestamp_initialization() {
    let mut bw_estimator = Estimator::default();

    let now = NoopClock.get_time();

    // Test that first_sent_time and delivered_time are updated if they are None
    assert_eq!(None, bw_estimator.state.first_sent_time);
    assert_eq!(None, bw_estimator.state.delivered_time);
    bw_estimator.on_packet_sent(false, false, now);
    assert_eq!(Some(now), bw_estimator.state.first_sent_time);
    assert_eq!(Some(now), bw_estimator.state.delivered_time);

    let new_now = now + Duration::from_secs(5);

    // Test that first_sent_time and delivered_time are not updated if packets are in flight
    bw_estimator.on_packet_sent(true, false, new_now);
    assert_eq!(Some(now), bw_estimator.state.first_sent_time);
    assert_eq!(Some(now), bw_estimator.state.delivered_time);

    // Test that first_sent_time and delivered_time are updated if packets are not in flight
    bw_estimator.on_packet_sent(false, false, new_now);
    assert_eq!(Some(new_now), bw_estimator.state.first_sent_time);
    assert_eq!(Some(new_now), bw_estimator.state.delivered_time);
}

#[test]
fn on_packet_sent_app_limited() {
    let mut bw_estimator = Estimator::default();

    let now = NoopClock.get_time();

    bw_estimator.on_packet_sent(false, false, now);
    assert_eq!(None, bw_estimator.state.app_limited_timestamp);

    bw_estimator.on_packet_sent(false, true, now);
    assert_eq!(Some(now), bw_estimator.state.app_limited_timestamp);
}

#[test]
fn on_packet_ack_resets_newly_acked_and_lost_on_new_ack() {
    let mut bw_estimator = Estimator::default();
    let now = NoopClock.get_time();
    bw_estimator.on_packet_sent(false, false, now);

    let mut packet = SentPacketInfo::new(
        true,
        1500,
        now,
        AckElicitation::Eliciting,
        path::Id::test_id(),
        ExplicitCongestionNotification::NotEct,
        bw_estimator.state(),
        0,
    );

    bw_estimator.on_packet_ack(&packet, now);
    assert_eq!(1500, bw_estimator.rate_sample.newly_acked_bytes);
    assert_eq!(1500, bw_estimator.state.delivered_bytes);
    assert_eq!(Some(now), bw_estimator.state.delivered_time);

    bw_estimator.on_packet_loss(1000);
    assert_eq!(1000, bw_estimator.rate_sample.newly_lost_bytes);

    // A packet is acked at the same time, this should add to newly_acked since it came
    // from the same ack frame
    bw_estimator.on_packet_ack(&packet, now);
    assert_eq!(3000, bw_estimator.rate_sample.newly_acked_bytes);
    assert_eq!(1000, bw_estimator.rate_sample.newly_lost_bytes);
    assert_eq!(3000, bw_estimator.state.delivered_bytes);
    assert_eq!(Some(now), bw_estimator.state.delivered_time);

    // A packet is acked at a later time, this should reset newly_acked and newly_lost
    // since it must be from a new ack frame
    packet.sent_bytes = 100;
    let now = now + Duration::from_secs(5);
    bw_estimator.on_packet_ack(&packet, now);

    assert_eq!(100, bw_estimator.rate_sample.newly_acked_bytes);
    assert_eq!(0, bw_estimator.rate_sample.newly_lost_bytes);
    assert_eq!(3100, bw_estimator.state.delivered_bytes);
    assert_eq!(Some(now), bw_estimator.state.delivered_time);
}

#[test]
fn on_packet_ack_clears_app_limited_timestamp() {
    let mut bw_estimator = Estimator::default();
    let t0 = NoopClock.get_time();
    let t1 = t0 + Duration::from_secs(1);
    // A packet is sent while application limited
    bw_estimator.on_packet_sent(false, true, t0);
    // A packet is sent while not application limited
    bw_estimator.on_packet_sent(true, false, t1);

    assert_eq!(Some(t0), bw_estimator.state.app_limited_timestamp);

    let packet_1 = SentPacketInfo::new(
        true,
        1500,
        t0,
        AckElicitation::Eliciting,
        path::Id::test_id(),
        ExplicitCongestionNotification::NotEct,
        bw_estimator.state(),
        0,
    );

    let t2 = t0 + Duration::from_secs(2);

    // The same packet is acked, this shouldn't clear the app_limited_timestamp since
    // it was sent while app-limited, not after.
    bw_estimator.on_packet_ack(&packet_1, t2);
    assert_eq!(Some(t0), bw_estimator.state.app_limited_timestamp);

    let packet_2 = SentPacketInfo::new(
        true,
        1500,
        t1,
        AckElicitation::Eliciting,
        path::Id::test_id(),
        ExplicitCongestionNotification::NotEct,
        bw_estimator.state(),
        0,
    );

    // Now a packet that was sent after the app_limited_timestamp has been acked,
    // so the app_limited_timestamp is cleared
    bw_estimator.on_packet_ack(&packet_2, t2);
    assert_eq!(None, bw_estimator.state.app_limited_timestamp);
}

#[test]
fn on_packet_ack_rate_sample() {
    let mut bw_estimator = Estimator::default();
    let t0 = NoopClock.get_time() + Duration::from_secs(60);
    bw_estimator.on_packet_sent(false, false, t0);

    let mut packet = SentPacketInfo::new(
        true,
        1500,
        t0,
        AckElicitation::Eliciting,
        path::Id::test_id(),
        ExplicitCongestionNotification::NotEct,
        bw_estimator.state(),
        500,
    );
    packet.lost_bytes = 1000;

    let t1 = t0 + Duration::from_secs(1);
    bw_estimator.on_packet_ack(&packet, t1);

    assert_eq!(bw_estimator.state.delivered_bytes, packet.sent_bytes as u64);
    assert_eq!(
        bw_estimator.rate_sample.newly_acked_bytes,
        packet.sent_bytes as u64
    );

    // Rate sample is updated since this is the first packet delivered
    assert_eq!(
        packet.delivered_bytes,
        bw_estimator.rate_sample.prior_delivered_bytes
    );
    assert_eq!(packet.lost_bytes, bw_estimator.rate_sample.prior_lost_bytes);
    assert_eq!(
        packet.is_app_limited,
        bw_estimator.rate_sample.is_app_limited
    );
    assert_eq!(
        packet.bytes_in_flight,
        bw_estimator.rate_sample.bytes_in_flight
    );
    assert_eq!(Some(packet.time_sent), bw_estimator.state.first_sent_time);
    assert_eq!(t1 - t0, bw_estimator.rate_sample.interval);

    // Ack a newer packet
    let mut new_packet = packet.clone();
    new_packet.delivered_bytes = 1500;
    new_packet.lost_bytes = 1000;
    new_packet.time_sent = t1;
    new_packet.first_sent_time = t1;
    new_packet.is_app_limited = true;
    new_packet.bytes_in_flight = 500;

    bw_estimator.on_packet_ack(&new_packet, t1);

    assert_eq!(
        bw_estimator.state.delivered_bytes,
        (packet.sent_bytes + new_packet.sent_bytes) as u64
    );
    assert_eq!(
        bw_estimator.rate_sample.newly_acked_bytes,
        (packet.sent_bytes + new_packet.sent_bytes) as u64
    );

    // Rate sample is updated since this packet is newer
    assert_eq!(
        new_packet.delivered_bytes,
        bw_estimator.rate_sample.prior_delivered_bytes
    );
    assert_eq!(
        new_packet.lost_bytes,
        bw_estimator.rate_sample.prior_lost_bytes
    );
    assert_eq!(
        new_packet.is_app_limited,
        bw_estimator.rate_sample.is_app_limited
    );
    assert_eq!(
        new_packet.bytes_in_flight,
        bw_estimator.rate_sample.bytes_in_flight
    );
    assert_eq!(
        Some(new_packet.time_sent),
        bw_estimator.state.first_sent_time
    );
    assert_eq!(t1 - t0, bw_estimator.rate_sample.interval);

    // Ack an older packet
    let mut old_packet = new_packet.clone();
    old_packet.delivered_bytes = old_packet.delivered_bytes - 1;
    old_packet.time_sent = old_packet.time_sent - Duration::from_secs(1);

    bw_estimator.on_packet_ack(&packet, t1);

    assert_eq!(
        bw_estimator.state.delivered_bytes,
        (packet.sent_bytes + new_packet.sent_bytes + old_packet.sent_bytes) as u64
    );
    assert_eq!(
        bw_estimator.rate_sample.newly_acked_bytes,
        (packet.sent_bytes + new_packet.sent_bytes + old_packet.sent_bytes) as u64
    );

    // Rate sample is not updated since this packet is older than the current sample
    assert_eq!(
        new_packet.delivered_bytes,
        bw_estimator.rate_sample.prior_delivered_bytes
    );
    assert_eq!(
        new_packet.lost_bytes,
        bw_estimator.rate_sample.prior_lost_bytes
    );
    assert_eq!(
        new_packet.is_app_limited,
        bw_estimator.rate_sample.is_app_limited
    );
    assert_eq!(
        new_packet.bytes_in_flight,
        bw_estimator.rate_sample.bytes_in_flight
    );
    assert_eq!(
        Some(new_packet.time_sent),
        bw_estimator.state.first_sent_time
    );
    assert_eq!(t1 - t0, bw_estimator.rate_sample.interval);
}

//= https://tools.ietf.org/id/draft-cheng-iccrg-delivery-rate-estimation-02#2.2.4
//= type=test
//# Since it is physically impossible to have data delivered faster than it is sent
//# in a sustained fashion, when the estimator notices that the ack_rate for a flight
//# is faster than the send rate for the flight, it filters out the implausible ack_rate
//# by capping the delivery rate sample to be no higher than the send rate.
#[test]
fn on_packet_ack_implausible_ack_rate() {
    let mut bw_estimator = Estimator::default();
    let t0 = NoopClock.get_time();
    bw_estimator.on_packet_sent(false, false, t0);
    bw_estimator.state.delivered_time = Some(t0 + Duration::from_secs(4));

    let packet = SentPacketInfo::new(
        true,
        1500,
        t0 + Duration::from_secs(5),
        AckElicitation::Eliciting,
        path::Id::test_id(),
        ExplicitCongestionNotification::NotEct,
        bw_estimator.state(),
        500,
    );

    let now = packet.time_sent + Duration::from_secs(1);
    bw_estimator.on_packet_ack(&packet, now);

    let send_elapsed = packet.time_sent - packet.first_sent_time;
    let ack_elapsed = now - packet.delivered_time;

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

    assert_eq!(0, bw_estimator.state.lost_bytes);
    assert_eq!(0, bw_estimator.rate_sample.newly_lost_bytes);
    assert_eq!(0, bw_estimator.rate_sample.lost_bytes);

    bw_estimator.on_packet_loss(500);

    assert_eq!(500, bw_estimator.state.lost_bytes);
    assert_eq!(500, bw_estimator.rate_sample.newly_lost_bytes);
    assert_eq!(500, bw_estimator.rate_sample.lost_bytes);

    bw_estimator.on_packet_loss(250);

    assert_eq!(750, bw_estimator.state.lost_bytes);
    assert_eq!(750, bw_estimator.rate_sample.newly_lost_bytes);
    assert_eq!(750, bw_estimator.rate_sample.lost_bytes);

    // Simulate a new ACK arriving, this would reset newly_lost and set prior_lost
    bw_estimator.rate_sample.newly_lost_bytes = 0;
    bw_estimator.rate_sample.prior_lost_bytes = 750;

    bw_estimator.on_packet_loss(250);

    assert_eq!(1000, bw_estimator.state.lost_bytes);
    assert_eq!(250, bw_estimator.rate_sample.newly_lost_bytes);
    assert_eq!(250, bw_estimator.rate_sample.lost_bytes);
}
