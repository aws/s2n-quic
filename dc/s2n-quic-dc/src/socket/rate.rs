// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Token-bucket rate limiting for pacing packet transmissions.

use crate::time::precision::Timestamp;

/// A token-bucket rate limiter for pacing packet transmissions.
///
/// Tokens are added at a constant rate (based on the configured throughput).
/// Each send consumes tokens proportional to the packet size. When the bucket
/// is empty, the sender sleeps until enough tokens accumulate. The bucket
/// has a bounded capacity to allow microbursts — if the sender was idle, it
/// can burst up to `burst_nanos` worth of data at line rate before pacing
/// kicks in.
#[derive(Clone, Copy)]
pub struct Rate {
    /// Nanoseconds per byte at the configured rate.
    nanos_per_byte: f64,
    /// Maximum burst allowance in nanoseconds of credit.
    /// This allows microbursts after idle periods.
    burst_nanos: u64,
}

impl Rate {
    pub fn new(gigabits_per_second: f64) -> Self {
        // nanos/byte = 8 / Gbps
        let nanos_per_byte = 8.0 / gigabits_per_second;

        // Allow up to 64KB worth of burst (1x GSO batch)
        // Reduced from 256KB to avoid overwhelming NIC TX ring
        let burst_nanos = (u16::MAX as f64 * nanos_per_byte) as u64;

        Self {
            nanos_per_byte,
            burst_nanos,
        }
    }

    /// Returns the number of nanoseconds to sleep after sending `bytes`.
    pub fn nanos_for_bytes(&self, bytes: u64) -> u64 {
        (bytes as f64 * self.nanos_per_byte) as u64
    }

    /// Returns the burst capacity in nanoseconds.
    pub fn burst_nanos(&self) -> u64 {
        self.burst_nanos
    }
}

impl core::fmt::Debug for Rate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let gigabits_per_second = 8.0 / self.nanos_per_byte;
        write!(f, "{:0.1}Gbps", gigabits_per_second)
    }
}

/// Token bucket state for pacing.
pub struct TokenBucket {
    /// Timestamp when the bucket was last refilled.
    last_refill: Timestamp,
    /// Available tokens in nanoseconds. Can go negative (debt).
    tokens_nanos: i64,
    /// Maximum tokens (burst capacity) in nanoseconds.
    capacity_nanos: i64,
}

impl TokenBucket {
    pub fn new(now: Timestamp, rate: &Rate) -> Self {
        Self {
            last_refill: now,
            tokens_nanos: rate.burst_nanos as i64,
            capacity_nanos: rate.burst_nanos as i64,
        }
    }

    /// Refill tokens based on elapsed time, then consume `cost_nanos`.
    /// Returns the number of nanos to sleep (0 if tokens are available).
    pub fn consume(&mut self, now: Timestamp, cost_nanos: u64) -> u64 {
        // Refill: add tokens for elapsed time since last refill
        let elapsed = now.nanos_since(self.last_refill);
        self.last_refill = now;
        self.tokens_nanos = (self.tokens_nanos + elapsed as i64).min(self.capacity_nanos);

        // Consume tokens
        self.tokens_nanos -= cost_nanos as i64;

        // If we went negative, we need to sleep until tokens recover
        if self.tokens_nanos < 0 {
            (-self.tokens_nanos) as u64
        } else {
            0
        }
    }

    /// Returns the current token balance in nanoseconds.
    pub fn tokens_nanos(&self) -> i64 {
        self.tokens_nanos
    }
}
