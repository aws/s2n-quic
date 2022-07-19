// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::time::Timestamp;
use core::{cmp::max, time::Duration};
use num_rational::Ratio;

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

const MICRO_BITS_PER_BYTE: u64 = 8 * 1000000;

#[derive(Copy, Clone, Debug, Default, Eq, Ord, PartialOrd, PartialEq)]
pub struct Bandwidth {
    bits_per_second: u64,
}

impl Bandwidth {
    pub const ZERO: Bandwidth = Bandwidth { bits_per_second: 0 };

    pub const MAX: Bandwidth = Bandwidth {
        bits_per_second: u64::MAX,
    };

    /// Constructs a new `Bandwidth` with the given bytes per interval
    pub const fn new(bytes: u64, interval: Duration) -> Self {
        if interval.is_zero() {
            Bandwidth::ZERO
        } else {
            Self {
                // Prefer multiplying by MICRO_BITS_PER_BYTE first to avoid losing resolution
                bits_per_second: match bytes.checked_mul(MICRO_BITS_PER_BYTE) {
                    Some(micro_bits) => micro_bits / interval.as_micros() as u64,
                    None => {
                        // If that overflows, divide first by the interval
                        (bytes / interval.as_micros() as u64).saturating_mul(MICRO_BITS_PER_BYTE)
                    }
                },
            }
        }
    }
}

impl core::ops::Mul<Ratio<u64>> for Bandwidth {
    type Output = Bandwidth;

    fn mul(self, rhs: Ratio<u64>) -> Self::Output {
        Bandwidth {
            bits_per_second: (rhs * self.bits_per_second).to_integer(),
        }
    }
}

impl core::ops::Mul<Duration> for Bandwidth {
    type Output = u64;

    fn mul(self, rhs: Duration) -> Self::Output {
        // Prefer multiplying by the duration first to avoid losing resolution
        match self.bits_per_second.checked_mul(rhs.as_micros() as u64) {
            Some(micro_bits) => micro_bits / MICRO_BITS_PER_BYTE,
            None => {
                // If that overflows, divide first by MICRO_BITS_PER_BYTE
                (self.bits_per_second / MICRO_BITS_PER_BYTE).saturating_mul(rhs.as_micros() as u64)
            }
        }
    }
}

/// Divides a count of bytes represented as a u64 by the given `Bandwidth`
///
/// Since `Bandwidth` is a rate of bytes over a time period, this division
/// results in a `Duration` being returned, representing how long a path
/// with the given `Bandwidth` would take to transmit the given number of
/// bytes.
impl core::ops::Div<Bandwidth> for u64 {
    type Output = Duration;

    fn div(self, rhs: Bandwidth) -> Self::Output {
        Duration::from_micros(self * MICRO_BITS_PER_BYTE / rhs.bits_per_second)
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
    /// The `delivered_bytes` that marks the end of the current application-limited period, or
    /// `None` if the connection is not currently application-limited.
    app_limited_delivered_bytes: Option<u64>,
    rate_sample: RateSample,
}

impl Estimator {
    /// Gets the total amount of data in bytes delivered so far over the lifetime of the path, not including
    /// non-congestion-controlled packets such as pure ACK packets.
    pub fn delivered_bytes(&self) -> u64 {
        self.delivered_bytes
    }

    /// Gets the total amount of data in bytes lost so far over the lifetime of the path
    pub fn lost_bytes(&self) -> u64 {
        self.lost_bytes
    }

    /// Gets the latest [RateSample]
    pub fn rate_sample(&self) -> RateSample {
        self.rate_sample
    }

    /// Returns true if the path is currently in an application-limited period
    pub fn is_app_limited(&self) -> bool {
        self.app_limited_delivered_bytes.is_some()
    }

    /// Called when a packet is transmitted
    pub fn on_packet_sent(
        &mut self,
        bytes_in_flight: u32,
        app_limited: Option<bool>,
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

        if app_limited.unwrap_or(false) {
            self.on_app_limited(bytes_in_flight);
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
            is_app_limited: self.app_limited_delivered_bytes.is_some(),
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

        if self
            .app_limited_delivered_bytes
            .map_or(false, |app_limited_bytes| {
                self.delivered_bytes > app_limited_bytes
            })
        {
            // Clear app-limited field if bubble is ACKed and gone
            self.app_limited_delivered_bytes = None;
        }

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

    /// Mark the path as app limited until the given `bytes_in_flight` are acknowledged
    pub fn on_app_limited(&mut self, bytes_in_flight: u32) {
        self.app_limited_delivered_bytes = Some(self.delivered_bytes + bytes_in_flight as u64);
    }
}

#[cfg(test)]
mod tests;
