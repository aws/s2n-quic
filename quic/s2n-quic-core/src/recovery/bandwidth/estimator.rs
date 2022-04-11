// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{number::Fraction, time::Timestamp};
use core::{cmp::max, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// Bandwidth-related data tracked for each sent packet
pub struct PacketInfo {
    /// [Estimator::delivered_bytes] at the time this packet was sent.
    pub delivered_bytes: u64,
    /// `Estimator::delivered_time` at the time this packet was sent.
    pub delivered_time: Timestamp,
    /// `Estimator::lost_bytes` at the time this packet was sent.
    pub lost_bytes: u64,
    /// `Estimator::first_sent_time` at the time this packet was sent.
    pub first_sent_time: Timestamp,
    /// The volume of data that was estimated to be in flight at the
    /// time of the transmission of this packet
    pub bytes_in_flight: u32,
    /// Whether the path send rate was limited by the application rather
    /// than congestion control at the time this packet was sent
    pub is_app_limited: bool,
}

#[derive(Copy, Clone, Debug, Default, PartialOrd, PartialEq)]
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

impl core::ops::Mul<Fraction> for Bandwidth {
    type Output = Bandwidth;

    fn mul(self, rhs: Fraction) -> Self::Output {
        Bandwidth {
            bits_per_second: self.bits_per_second * rhs.numerator() as u64
                / rhs.denominator() as u64,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
/// A bandwidth delivery rate estimate with associated metadata
pub struct RateSample {
    /// The length of the sampling interval
    pub interval: Duration,
    /// The amount of data in bytes marked as delivered over the sampling interval
    pub delivered_bytes: u64,
    /// The amount of data in bytes marked as lost over the sampling interval
    pub lost_bytes: u64,
    /// [PacketInfo::is_app_limited] from the most recent acknowledged packet
    pub is_app_limited: bool,
    /// [PacketInfo::delivered_bytes] from the most recent acknowledged packet
    pub prior_delivered_bytes: u64,
    /// [PacketInfo::bytes_in_flight] from the most recent acknowledged packet
    pub bytes_in_flight: u32,
    /// [PacketInfo::lost_bytes] from the most recent acknowledged packet
    pub prior_lost_bytes: u64,
}

impl RateSample {
    /// Updates the rate sample with the most recent acknowledged packet
    fn on_ack(&mut self, packet_info: PacketInfo) {
        debug_assert!(
            packet_info.delivered_bytes >= self.prior_delivered_bytes,
            "on_ack should only be called with the newest acknowledged packet"
        );

        self.is_app_limited = packet_info.is_app_limited;
        self.prior_delivered_bytes = packet_info.delivered_bytes;
        self.prior_lost_bytes = packet_info.lost_bytes;
        self.bytes_in_flight = packet_info.bytes_in_flight;
    }

    /// Gets the delivery rate of this rate sample
    pub fn delivery_rate(&self) -> Bandwidth {
        Bandwidth::new(self.delivered_bytes, self.interval)
    }
}

/// Bandwidth estimator as defined in [Delivery Rate Estimation](https://datatracker.ietf.org/doc/draft-cheng-iccrg-delivery-rate-estimation/)
/// and [BBR Congestion Control](https://datatracker.ietf.org/doc/draft-cardwell-iccrg-bbr-congestion-control/).
#[derive(Clone, Debug, Default)]
pub struct Estimator {
    //= https://tools.ietf.org/id/draft-cheng-iccrg-delivery-rate-estimation-02#2.2
    //# The amount of data delivered MAY be tracked in units of either octets or packets.
    /// The total amount of data in bytes delivered so far over the lifetime of the path, not including
    /// non-congestion-controlled packets such as pure ACK packets.
    delivered_bytes: u64,
    /// The timestamp when delivered_bytes was last updated, or if the connection
    /// was recently idle, the send time of the first packet sent after resuming from idle.
    delivered_time: Option<Timestamp>,
    /// The total amount of data in bytes declared lost so far over the lifetime of the path, not including
    /// non-congestion-controlled packets such as pure ACK packets.
    lost_bytes: u64,
    /// The send time of the packet that was most recently marked as delivered, or if the connection
    /// was recently idle, the send time of the first packet sent after resuming from idle.
    first_sent_time: Option<Timestamp>,
    rate_sample: RateSample,
}

impl Estimator {
    /// Gets the total amount of data in bytes delivered so far over the lifetime of the path, not including
    /// non-congestion-controlled packets such as pure ACK packets.
    pub fn delivered_bytes(&self) -> u64 {
        self.delivered_bytes
    }

    /// Gets the latest [RateSample]
    pub fn rate_sample(&self) -> RateSample {
        self.rate_sample
    }

    /// Called when a packet is transmitted
    pub fn on_packet_sent(
        &mut self,
        bytes_in_flight: u32,
        is_app_limited: bool,
        now: Timestamp,
    ) -> PacketInfo {
        //= https://tools.ietf.org/id/draft-cheng-iccrg-delivery-rate-estimation-02#3.2
        //# If there are no packets in flight yet, then we can start the delivery rate interval
        //# at the current time, since we know that any ACKs after now indicate that the network
        //# was able to deliver those packets completely in the sampling interval between now
        //# and the next ACK.
        if bytes_in_flight == 0 {
            self.first_sent_time = Some(now);
            self.delivered_time = Some(now);
        }

        PacketInfo {
            delivered_bytes: self.delivered_bytes,
            delivered_time: self
                .delivered_time
                .expect("initialized on first sent packet"),
            lost_bytes: self.lost_bytes,
            first_sent_time: self
                .first_sent_time
                .expect("initialized on first sent packet"),
            bytes_in_flight,
            is_app_limited,
        }
    }

    //= https://tools.ietf.org/id/draft-cheng-iccrg-delivery-rate-estimation-02#3.3
    //# For each packet that was newly SACKed or ACKed, UpdateRateSample() updates the
    //# rate sample based on a snapshot of connection delivery information from the time
    //# at which the packet was last transmitted.
    /// Called for each acknowledgement of one or more packets
    pub fn on_ack(
        &mut self,
        bytes_acknowledged: usize,
        newest_acked_time_sent: Timestamp,
        newest_acked_packet_info: PacketInfo,
        now: Timestamp,
    ) {
        self.delivered_bytes += bytes_acknowledged as u64;
        self.delivered_time = Some(now);

        //= https://tools.ietf.org/id/draft-cheng-iccrg-delivery-rate-estimation-02#3.3
        //# UpdateRateSample() is invoked multiple times when a stretched ACK acknowledges
        //# multiple data packets. In this case we use the information from the most recently
        //# sent packet, i.e., the packet with the highest "P.delivered" value.
        if self.rate_sample.prior_delivered_bytes == 0
            || newest_acked_packet_info.delivered_bytes > self.rate_sample.prior_delivered_bytes
        {
            // Update info using the newest packet
            self.rate_sample.on_ack(newest_acked_packet_info);
            self.first_sent_time = Some(newest_acked_time_sent);

            let send_elapsed = newest_acked_time_sent - newest_acked_packet_info.first_sent_time;
            let ack_elapsed = now - newest_acked_packet_info.delivered_time;

            //= https://tools.ietf.org/id/draft-cheng-iccrg-delivery-rate-estimation-02#2.2.4
            //# Since it is physically impossible to have data delivered faster than it is sent
            //# in a sustained fashion, when the estimator notices that the ack_rate for a flight
            //# is faster than the send rate for the flight, it filters out the implausible ack_rate
            //# by capping the delivery rate sample to be no higher than the send rate.
            self.rate_sample.interval = max(send_elapsed, ack_elapsed);
        }

        self.rate_sample.delivered_bytes =
            self.delivered_bytes - self.rate_sample.prior_delivered_bytes;
    }

    /// Called when packets are declared lost
    #[inline]
    pub fn on_loss(&mut self, lost_bytes: usize) {
        self.lost_bytes += lost_bytes as u64;
        self.rate_sample.lost_bytes = self.lost_bytes - self.rate_sample.prior_lost_bytes;
    }
}

#[cfg(test)]
mod tests;
