// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::time::Timestamp;
use core::{cmp::max, time::Duration};

#[derive(Default)]
pub struct Bandwidth {
    #[allow(dead_code)] // TODO: Remove when incorporated into BBR
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
    bytes_in_flight: u32,
    /// The amount of data in bytes marked as lost over the sampling interval
    lost: u64,
    /// A snapshot of the total data in bytes lost over the lifetime of the connection
    /// at the time the most recently acknowledged packet was sent
    prior_lost: u64,
}

impl RateSample {
    /// The delivery rate sample
    pub fn delivery_rate(&self) -> Bandwidth {
        Bandwidth::new(self.delivered, self.interval)
    }
}

/// Bandwidth estimator as defined in https://datatracker.ietf.org/doc/draft-cheng-iccrg-delivery-rate-estimation/
/// and https://datatracker.ietf.org/doc/draft-cardwell-iccrg-bbr-congestion-control/.
#[derive(Clone, Debug, Default)]
pub struct BandwidthEstimator {
    delivered_bytes: u64,
    delivered_time: Option<Timestamp>,
    lost_bytes: u64,
    first_sent_time: Option<Timestamp>,
    rate_sample: RateSample,
    app_limited_timestamp: Option<Timestamp>,
}

impl BandwidthEstimator {
    /// The total amount of data in bytes delivered so far over the lifetime of the path, not including
    /// non-congestion-controlled packets such as pure ACK packets.
    pub fn delivered_bytes(&self) -> u64 {
        self.delivered_bytes
    }

    /// The timestamp when delivered_bytes was last updated, or the time the first packet was
    /// sent if no packet was in flight yet.
    pub fn delivered_time(&self) -> Option<Timestamp> {
        self.delivered_time
    }

    /// The total amount of data in bytes declared lost so far over the lifetime of the path, not including
    /// non-congestion-controlled packets such as pure ACK packets.
    pub fn lost_bytes(&self) -> u64 {
        self.lost_bytes
    }

    /// If packets are in flight, then this holds the send time of the packet that was most recently
    /// marked as delivered. Else, if the connection was recently idle, then this holds the send
    /// time of the first packet sent after resuming from idle.
    pub fn first_sent_time(&self) -> Option<Timestamp> {
        self.first_sent_time
    }

    /// The time sent of the last transmitted packet marked as application-limited
    pub fn app_limited_timestamp(&self) -> Option<Timestamp> {
        self.app_limited_timestamp
    }

    /// Called when a packet is transmitted
    pub fn on_packet_sent(
        &mut self,
        packets_in_flight: bool,
        application_limited: bool,
        now: Timestamp,
    ) {
        if !packets_in_flight || self.first_sent_time.is_none() || self.delivered_time.is_none() {
            self.first_sent_time = Some(now);
            self.delivered_time = Some(now);
        }

        if application_limited {
            self.app_limited_timestamp = Some(now);
        }
    }

    /// Called for each newly acknowledged packet
    pub fn on_packet_ack(
        &mut self,
        delivered: u64,
        delivered_time: Timestamp,
        lost_bytes: u64,
        time_sent: Timestamp,
        first_sent_time: Timestamp,
        app_limited: bool,
        bytes_in_flight: u32,
        sent_bytes: usize,
        now: Timestamp,
    ) {
        if self
            .delivered_time
            .map_or(true, |delivered_time| now > delivered_time)
        {
            // This is the first ack from a new ACK frame, reset newly acked and lost
            self.rate_sample.newly_acked = 0;
            self.rate_sample.newly_lost = 0;
        }

        self.delivered_bytes += sent_bytes as u64;
        self.delivered_time = Some(now);
        self.rate_sample.newly_acked += sent_bytes as u64;

        if delivered > self.rate_sample.prior_delivered {
            // Update info using the newest packet
            self.rate_sample.prior_delivered = delivered;
            self.rate_sample.prior_lost = lost_bytes;
            self.rate_sample.is_app_limited = app_limited;
            self.rate_sample.bytes_in_flight = bytes_in_flight;
            self.first_sent_time = Some(time_sent);

            let send_elapsed = time_sent - first_sent_time;
            let ack_elapsed = now - delivered_time;
            /* Use the longer of the send_elapsed and ack_elapsed */
            self.rate_sample.interval = max(send_elapsed, ack_elapsed);

            let sent_after_app_limited = self
                .app_limited_timestamp
                .map_or(false, |app_limited_timestamp| {
                    time_sent > app_limited_timestamp
                });
            if sent_after_app_limited {
                // The connection is no longer app limited if packets that were sent after the time
                // the connection was app limited have been acknowledged.
                self.app_limited_timestamp = None;
            }
        }

        self.rate_sample.delivered = self.delivered_bytes - self.rate_sample.prior_delivered;
    }

    /// Called when a packet is declared lost
    #[inline]
    pub fn on_packet_loss(&mut self, lost_bytes: usize) {
        self.lost_bytes += lost_bytes as u64;
        self.rate_sample.newly_lost += lost_bytes as u64;
        self.rate_sample.lost = self.lost_bytes - self.rate_sample.prior_lost;
    }
}

#[cfg(test)]
mod tests;
