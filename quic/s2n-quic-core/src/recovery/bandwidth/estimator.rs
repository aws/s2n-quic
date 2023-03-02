// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{event, event::IntoEvent, recovery::congestion_controller::Publisher, time::Timestamp};
use core::{
    cmp::{max, Ordering},
    time::Duration,
};
use num_rational::Ratio;
use num_traits::Inv;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// Bandwidth-related data tracked for each sent packet
pub struct PacketInfo {
    /// [Estimator::delivered_bytes] at the time this packet was sent.
    pub delivered_bytes: u64,
    /// `Estimator::delivered_time` at the time this packet was sent.
    pub delivered_time: Timestamp,
    /// `Estimator::lost_bytes` at the time this packet was sent.
    pub lost_bytes: u64,
    /// `Estimator::ecn_ce_count` at the time this packet was sent.
    pub ecn_ce_count: u64,
    /// `Estimator::first_sent_time` at the time this packet was sent.
    pub first_sent_time: Timestamp,
    /// The volume of data that was estimated to be in flight at the
    /// time of the transmission of this packet
    pub bytes_in_flight: u32,
    /// Whether the path send rate was limited by the application rather
    /// than congestion control at the time this packet was sent
    pub is_app_limited: bool,
}

/// Represents a rate at which data is transferred
///
/// While bandwidth is typically thought of as an amount of data over a fixed
/// amount of time (bytes per second, for example), in this case we internally
/// represent bandwidth as the inverse: an amount of time to send a fixed amount
/// of data (nanoseconds per kibibyte or 1024 bytes, in this case). This allows for
/// some of the math operations needed on `Bandwidth` to avoid division, while
/// reducing the likelihood of panicking due to overflow.
///
/// The maximum (non-infinite) value that can be represented is ~1 TB/second.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Bandwidth {
    nanos_per_kibibyte: u64,
}

// 2^10 = 1024 bytes in kibibyte
const KIBIBYTE_SHIFT: u8 = 10;

impl Bandwidth {
    pub const ZERO: Bandwidth = Bandwidth {
        nanos_per_kibibyte: u64::MAX,
    };

    pub const INFINITY: Bandwidth = Bandwidth {
        nanos_per_kibibyte: 0,
    };

    /// Constructs a new `Bandwidth` with the given bytes per interval
    pub const fn new(bytes: u64, interval: Duration) -> Self {
        let interval = (interval.as_nanos() as u64) << KIBIBYTE_SHIFT;
        if interval == 0 || bytes == 0 {
            Bandwidth::ZERO
        } else {
            Self {
                nanos_per_kibibyte: interval / bytes,
            }
        }
    }

    /// Represents the bandwidth as bytes per second
    pub fn as_bytes_per_second(&self) -> u64 {
        const ONE_SECOND_IN_NANOS: u64 = Duration::from_secs(1).as_nanos() as u64;

        if *self == Bandwidth::INFINITY {
            return u64::MAX;
        }

        (ONE_SECOND_IN_NANOS << KIBIBYTE_SHIFT) / self.nanos_per_kibibyte
    }
}

impl Default for Bandwidth {
    fn default() -> Self {
        Bandwidth::ZERO
    }
}

impl core::cmp::PartialOrd for Bandwidth {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl core::cmp::Ord for Bandwidth {
    fn cmp(&self, other: &Self) -> Ordering {
        // The higher the nanos_per_kibibyte, the lower the bandwidth,
        // so reverse the ordering when comparing
        self.nanos_per_kibibyte
            .cmp(&other.nanos_per_kibibyte)
            .reverse()
    }
}

impl core::ops::Mul<Ratio<u64>> for Bandwidth {
    type Output = Bandwidth;

    fn mul(self, rhs: Ratio<u64>) -> Self::Output {
        if self == Bandwidth::ZERO {
            return Bandwidth::ZERO;
        }

        Bandwidth {
            // Since `Bandwidth` is represented as time/byte and not bytes/time, we should divide
            // by the given ratio to result in a higher nanos_per_kibibyte value (lower bandwidth).
            // To avoid division, we can multiply by the inverse of the ratio instead
            nanos_per_kibibyte: (rhs.inv() * self.nanos_per_kibibyte).to_integer(),
        }
    }
}

impl core::ops::Mul<Duration> for Bandwidth {
    type Output = u64;

    fn mul(self, rhs: Duration) -> Self::Output {
        if self == Bandwidth::INFINITY {
            return u64::MAX;
        } else if rhs.is_zero() {
            return 0;
        }

        let interval = (rhs.as_nanos() as u64) << KIBIBYTE_SHIFT;

        if interval == 0 {
            return u64::MAX;
        }

        interval / self.nanos_per_kibibyte
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
        Duration::from_nanos(rhs.nanos_per_kibibyte.saturating_mul(self) >> KIBIBYTE_SHIFT)
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
    /// The number of packets marked as explicit congestion experienced over the sampling interval
    pub ecn_ce_count: u64,
    /// [PacketInfo::is_app_limited] from the most recent acknowledged packet
    pub is_app_limited: bool,
    /// [PacketInfo::delivered_bytes] from the most recent acknowledged packet
    pub prior_delivered_bytes: u64,
    /// [PacketInfo::bytes_in_flight] from the most recent acknowledged packet
    pub bytes_in_flight: u32,
    /// [PacketInfo::lost_bytes] from the most recent acknowledged packet
    pub prior_lost_bytes: u64,
    /// [PacketInfo::ecn_ce_count] from the most recent acknowledged packet
    pub prior_ecn_ce_count: u64,
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
        self.prior_ecn_ce_count = packet_info.ecn_ce_count;
        self.bytes_in_flight = packet_info.bytes_in_flight;
    }

    /// Gets the delivery rate of this rate sample
    pub fn delivery_rate(&self) -> Bandwidth {
        Bandwidth::new(self.delivered_bytes, self.interval)
    }
}

impl IntoEvent<event::builder::RateSample> for RateSample {
    fn into_event(self) -> event::builder::RateSample {
        event::builder::RateSample {
            interval: self.interval.into_event(),
            delivered_bytes: self.delivered_bytes.into_event(),
            lost_bytes: self.lost_bytes.into_event(),
            ecn_ce_count: self.ecn_ce_count.into_event(),
            is_app_limited: self.is_app_limited.into_event(),
            prior_delivered_bytes: self.prior_delivered_bytes.into_event(),
            bytes_in_flight: self.bytes_in_flight.into_event(),
            prior_lost_bytes: self.prior_lost_bytes.into_event(),
            prior_ecn_ce_count: self.prior_ecn_ce_count.into_event(),
            delivery_rate_bytes_per_second: self.delivery_rate().as_bytes_per_second().into_event(),
        }
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
    /// The total amount of explicit congestion experienced packets over the lifetime of the path
    ecn_ce_count: u64,
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
        prior_bytes_in_flight: u32,
        sent_bytes: usize,
        app_limited: Option<bool>,
        now: Timestamp,
    ) -> PacketInfo {
        //= https://tools.ietf.org/id/draft-cheng-iccrg-delivery-rate-estimation-02#3.2
        //# If there are no packets in flight yet, then we can start the delivery rate interval
        //# at the current time, since we know that any ACKs after now indicate that the network
        //# was able to deliver those packets completely in the sampling interval between now
        //# and the next ACK.

        //= https://tools.ietf.org/id/draft-cheng-iccrg-delivery-rate-estimation-02#3.2
        //# Upon each packet transmission, the sender executes the following steps:
        //#
        //# SendPacket(Packet P):
        //#   if (SND.NXT == SND.UNA)  /* no packets in flight yet? */
        //#     C.first_sent_time  = C.delivered_time = Now()
        //#   P.first_sent_time = C.first_sent_time
        //#   P.delivered_time  = C.delivered_time
        //#   P.delivered       = C.delivered
        //#   P.is_app_limited  = (C.app_limited != 0)
        if prior_bytes_in_flight == 0 {
            self.first_sent_time = Some(now);
            self.delivered_time = Some(now);
        }

        let bytes_in_flight = prior_bytes_in_flight.saturating_add(sent_bytes as u32);

        if app_limited.unwrap_or(true) {
            self.on_app_limited(bytes_in_flight);
        }

        PacketInfo {
            delivered_bytes: self.delivered_bytes,
            delivered_time: self
                .delivered_time
                .expect("initialized on first sent packet"),
            lost_bytes: self.lost_bytes,
            ecn_ce_count: self.ecn_ce_count,
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
    pub fn on_ack<Pub: Publisher>(
        &mut self,
        bytes_acknowledged: usize,
        newest_acked_time_sent: Timestamp,
        newest_acked_packet_info: PacketInfo,
        now: Timestamp,
        publisher: &mut Pub,
    ) {
        self.delivered_bytes += bytes_acknowledged as u64;
        self.delivered_time = Some(now);

        let is_app_limited_period_over =
            |app_limited_bytes| self.delivered_bytes > app_limited_bytes;
        if self
            .app_limited_delivered_bytes
            .map_or(false, is_app_limited_period_over)
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
        // Lost bytes and ECN CE Count are updated here as well as in `on_loss` and `on_explicit_congestion`
        // so the values are up to date even when no loss or ECN CE markings are received.
        self.rate_sample.lost_bytes = self.lost_bytes - self.rate_sample.prior_lost_bytes;
        self.rate_sample.ecn_ce_count = self.ecn_ce_count - self.rate_sample.prior_ecn_ce_count;

        publisher.on_delivery_rate_sampled(self.rate_sample);
    }

    /// Called when packets are declared lost
    #[inline]
    pub fn on_loss(&mut self, lost_bytes: usize) {
        self.lost_bytes += lost_bytes as u64;
        self.rate_sample.lost_bytes = self.lost_bytes - self.rate_sample.prior_lost_bytes;

        // Move the app-limited period earlier as the lost bytes will not be delivered
        if let Some(ref mut app_limited_delivered_bytes) = self.app_limited_delivered_bytes {
            *app_limited_delivered_bytes =
                app_limited_delivered_bytes.saturating_sub(lost_bytes as u64)
        }
    }

    /// Called when packets are discarded
    #[inline]
    pub fn on_packet_discarded(&mut self, bytes_sent: usize) {
        // Move the app-limited period earlier as the discarded bytes will not be delivered
        if let Some(ref mut app_limited_delivered_bytes) = self.app_limited_delivered_bytes {
            *app_limited_delivered_bytes =
                app_limited_delivered_bytes.saturating_sub(bytes_sent as u64)
        }
    }

    /// Called each time explicit congestion is recorded
    #[inline]
    pub fn on_explicit_congestion(&mut self, ce_count: u64) {
        self.ecn_ce_count += ce_count;
        self.rate_sample.ecn_ce_count = self.ecn_ce_count - self.rate_sample.prior_ecn_ce_count;
    }

    /// Mark the path as app limited until the given `bytes_in_flight` are acknowledged
    pub fn on_app_limited(&mut self, bytes_in_flight: u32) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.4.4
        //# MarkConnectionAppLimited():
        //#   C.app_limited =
        //#     (C.delivered + packets_in_flight) ? : 1
        self.app_limited_delivered_bytes = Some(self.delivered_bytes + bytes_in_flight as u64);
    }

    #[cfg(test)]
    pub fn set_rate_sample_for_test(&mut self, rate_sample: RateSample) {
        self.rate_sample = rate_sample
    }

    #[cfg(test)]
    pub fn set_delivered_bytes_for_test(&mut self, delivered_bytes: u64) {
        self.delivered_bytes = delivered_bytes
    }
}

#[cfg(test)]
mod tests;
