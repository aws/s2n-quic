// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::time::Timestamp;
use core::{cmp::max, num::NonZeroU64, time::Duration};

#[derive(Default)]
pub struct Bandwidth {
    bits_per_second: u64,
}

impl Bandwidth {
    pub const ZERO: Bandwidth = Bandwidth { bits_per_second: 0 };

    pub fn new(bytes: u64, interval: Duration) -> Self {
        const MICRO_BITS_PER_BYTE: u64 = 8 * 1000000;

        if interval.is_zero() {
            Bandwidth::ZERO
        } else {
            Self {
                bits_per_second: (bytes * MICRO_BITS_PER_BYTE / interval.as_micros() as u64),
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct RateSample {
    /// Whether this rate sample should be considered application limited
    ///
    /// True if the connection was application limited at the time the most recently acknowledged
    /// packet was sent.
    is_app_limited: bool,
    /// The length of the sampling interval
    interval: Duration,
    /// The amount of data in bytes marked as delivered over the sampling interval
    delivered: u64,
    /// A snapshot of the total data in bytes delivered over the lifetime of the connection
    /// at the time the most recently acknowledged packet was sent
    prior_delivered: u64,
    /// The number of bytes of data acknowledged in the latest ACK frame
    newly_acked: u64,
    /// The number of bytes of data declared lost upon receipt of the latest ACK frame
    newly_lost: u64,
    /// The number of bytes that was estimated to be in flight at the time of the transmission of
    /// the packet that has just been ACKed
    tx_in_flight: u64,
    /// The amount of data in bytes marked as lost over the sampling interval
    lost: u64,
    /// A snapshot of the total data in bytes lost over the lifetime of the connection
    /// at the time the most recently acknowledged packet was sent
    prior_lost: u64,
}

impl RateSample {
    /// The delivery rate sample
    fn delivery_rate(&self) -> Bandwidth {
        Bandwidth::new(self.delivered, self.interval)
    }
}

/// Bandwidth estimator as defined in https://datatracker.ietf.org/doc/draft-cheng-iccrg-delivery-rate-estimation/
/// and https://datatracker.ietf.org/doc/draft-cardwell-iccrg-bbr-congestion-control/.
#[derive(Clone, Debug, Default)]
pub struct BandwidthEstimator {
    delivered: u64,
    delivered_time: Option<Timestamp>,
    lost: u64,
    first_sent_time: Option<Timestamp>,
    rate_sample: RateSample,
    app_limited: Option<NonZeroU64>,
}

impl BandwidthEstimator {
    /// Called when a packet is transmitted with no packets
    /// currently in flight
    #[inline]
    pub fn on_resume_after_idle(&mut self, now: Timestamp) {
        self.first_sent_time = Some(now);
        self.delivered_time = Some(now);
    }

    /// Called for each newly acknowledged packet
    #[inline]
    pub fn on_packet_ack(&mut self, acked_bytes: u64, now: Timestamp) {
        self.delivered += acked_bytes;
        self.delivered_time = Some(now);
    }

    /// Called when a packet is declared lost
    #[inline]
    pub fn on_packet_loss(&mut self, lost_bytes: u64) {
        self.lost += lost_bytes;
    }

    /// Updates the bandwidth rate sample with data from a received acknowledgement
    ///
    /// Similar to the `rtt_estimator`, this should only be called when the the largest
    /// acknowledged packet number is newly acknowledged.
    pub fn update_rate_sample(
        &mut self,
        delivered: u64,
        delivered_time: Timestamp,
        acked_bytes: u64,
        lost_bytes: u64,
        time_sent: Timestamp,
        first_sent_time: Timestamp,
        app_limited: bool,
        tx_in_flight: u64,
    ) {
        debug_assert!(
            delivered > self.rate_sample.prior_delivered,
            "update_rate_sample should only be called for the largest newly acked packet"
        );

        self.rate_sample.prior_delivered = delivered;
        self.rate_sample.prior_lost = lost_bytes;
        self.rate_sample.is_app_limited = app_limited;
        self.first_sent_time = Some(time_sent);

        /* Clear app-limited field if bubble is ACKed and gone. */
        if self.app_limited.map_or(false, |app_limited_amt| {
            self.delivered > app_limited_amt.into()
        }) {
            self.app_limited = None;
        }

        let send_elapsed = time_sent - first_sent_time;
        let ack_elapsed = self
            .delivered_time
            .expect("delivered_time is populated by an ack before update_rate_sample is called")
            - delivered_time;
        /* Use the longer of the send_elapsed and ack_elapsed */
        self.rate_sample.interval = max(send_elapsed, ack_elapsed);

        self.rate_sample.delivered = self.delivered - self.rate_sample.prior_delivered;
        self.rate_sample.lost = self.lost - self.rate_sample.prior_lost;

        // acked_bytes is the latest total amount acknowledged in the ack frame
        self.rate_sample.newly_acked = acked_bytes;
        self.rate_sample.newly_lost = lost_bytes;
        self.rate_sample.tx_in_flight = tx_in_flight;
    }
}
