// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{recovery::SentPacketInfo, time::Timestamp};
use core::{cmp::max, time::Duration};

#[derive(Clone, Copy, Debug, Default)]
/// Bandwidth-related data tracked for each path
pub struct BandwidthState {
    //= https://tools.ietf.org/id/draft-cheng-iccrg-delivery-rate-estimation-02#2.2
    //# The amount of data delivered MAY be tracked in units of either octets or packets.
    delivered_bytes: u64,
    delivered_time: Option<Timestamp>,
    lost_bytes: u64,
    first_sent_time: Option<Timestamp>,
    app_limited_timestamp: Option<Timestamp>,
}

impl BandwidthState {
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
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RateSample {
    /// Whether this rate sample should be considered application limited
    ///
    /// True if the connection was application limited at the time the most recently acknowledged
    /// packet was sent.
    pub is_app_limited: bool,
    /// The length of the sampling interval
    pub interval: Duration,
    /// The amount of data in bytes marked as delivered over the sampling interval
    pub delivered_bytes: u64,
    /// A snapshot of the total data in bytes delivered over the lifetime of the connection
    /// at the time the most recently acknowledged packet was sent
    pub prior_delivered_bytes: u64,
    /// The number of bytes of data acknowledged in the latest ACK frame
    pub newly_acked_bytes: u64,
    /// The number of bytes of data declared lost upon receipt of the latest ACK frame
    pub newly_lost_bytes: u64,
    /// The number of bytes that was estimated to be in flight at the time of the transmission of
    /// the packet that has just been ACKed
    pub bytes_in_flight: u32,
    /// The amount of data in bytes marked as lost over the sampling interval
    pub lost_bytes: u64,
    /// A snapshot of the total data in bytes lost over the lifetime of the connection
    /// at the time the most recently acknowledged packet was sent
    pub prior_lost_bytes: u64,
}

/// Bandwidth estimator as defined in [Delivery Rate Estimation](https://datatracker.ietf.org/doc/draft-cheng-iccrg-delivery-rate-estimation/)
/// and [BBR Congestion Control](https://datatracker.ietf.org/doc/draft-cardwell-iccrg-bbr-congestion-control/).
#[derive(Clone, Debug, Default)]
pub struct BandwidthEstimator {
    state: BandwidthState,
    rate_sample: RateSample,
}

impl BandwidthEstimator {
    /// Gets the current bandwidth::State
    pub fn state(&self) -> BandwidthState {
        self.state
    }

    /// Gets the latest bandwidth:RateSample
    pub fn rate_sample(&self) -> RateSample {
        self.rate_sample
    }

    /// Called when a packet is transmitted
    pub fn on_packet_sent(
        &mut self,
        packets_in_flight: bool,
        application_limited: bool,
        now: Timestamp,
    ) {
        //= https://tools.ietf.org/id/draft-cheng-iccrg-delivery-rate-estimation-02#3.2
        //# If there are no packets in flight yet, then we can start the delivery rate interval
        //# at the current time, since we know that any ACKs after now indicate that the network
        //# was able to deliver those packets completely in the sampling interval between now
        //# and the next ACK.
        if !packets_in_flight
            || self.state.first_sent_time.is_none()
            || self.state.delivered_time.is_none()
        {
            self.state.first_sent_time = Some(now);
            self.state.delivered_time = Some(now);
        }

        if application_limited {
            self.state.app_limited_timestamp = Some(now);
        }
    }

    //= https://tools.ietf.org/id/draft-cheng-iccrg-delivery-rate-estimation-02#3.3
    //# For each packet that was newly SACKed or ACKed, UpdateRateSample() updates the
    //# rate sample based on a snapshot of connection delivery information from the time
    //# at which the packet was last transmitted.
    /// Called for each newly acknowledged packet
    pub fn on_packet_ack(&mut self, packet: &SentPacketInfo, now: Timestamp) {
        if self
            .state
            .delivered_time
            .map_or(true, |delivered_time| now > delivered_time)
        {
            // This is the first ack from a new ACK frame, reset newly acked and lost
            self.rate_sample.newly_acked_bytes = 0;
            self.rate_sample.newly_lost_bytes = 0;
        }

        self.state.delivered_bytes += packet.sent_bytes as u64;
        self.state.delivered_time = Some(now);
        self.rate_sample.newly_acked_bytes += packet.sent_bytes as u64;

        //= https://tools.ietf.org/id/draft-cheng-iccrg-delivery-rate-estimation-02#3.3
        //# UpdateRateSample() is invoked multiple times when a stretched ACK acknowledges
        //# multiple data packets. In this case we use the information from the most recently
        //# sent packet, i.e., the packet with the highest "P.delivered" value.
        if self.rate_sample.prior_delivered_bytes == 0
            || packet.delivered_bytes > self.rate_sample.prior_delivered_bytes
        {
            // Update info using the newest packet
            self.rate_sample.prior_delivered_bytes = packet.delivered_bytes;
            self.rate_sample.prior_lost_bytes = packet.lost_bytes;
            self.rate_sample.is_app_limited = packet.is_app_limited;
            self.rate_sample.bytes_in_flight = packet.bytes_in_flight;
            self.state.first_sent_time = Some(packet.time_sent);

            let send_elapsed = packet.time_sent - packet.first_sent_time;
            let ack_elapsed = now - packet.delivered_time;

            //= https://tools.ietf.org/id/draft-cheng-iccrg-delivery-rate-estimation-02#2.2.4
            //# Since it is physically impossible to have data delivered faster than it is sent
            //# in a sustained fashion, when the estimator notices that the ack_rate for a flight
            //# is faster than the send rate for the flight, it filters out the implausible ack_rate
            //# by capping the delivery rate sample to be no higher than the send rate.
            self.rate_sample.interval = max(send_elapsed, ack_elapsed);

            let sent_after_app_limited = self
                .state
                .app_limited_timestamp
                .map_or(false, |app_limited_timestamp| {
                    packet.time_sent > app_limited_timestamp
                });
            if sent_after_app_limited {
                // The connection is no longer app limited if packets that were sent after the time
                // the connection was app limited have been acknowledged.
                self.state.app_limited_timestamp = None;
            }
        }

        self.rate_sample.delivered_bytes =
            self.state.delivered_bytes - self.rate_sample.prior_delivered_bytes;
    }

    /// Called when a packet is declared lost
    #[inline]
    pub fn on_packet_loss(&mut self, lost_bytes: usize) {
        self.state.lost_bytes += lost_bytes as u64;
        self.rate_sample.newly_lost_bytes += lost_bytes as u64;
        self.rate_sample.lost_bytes = self.state.lost_bytes - self.rate_sample.prior_lost_bytes;
    }
}

#[cfg(test)]
mod tests;
