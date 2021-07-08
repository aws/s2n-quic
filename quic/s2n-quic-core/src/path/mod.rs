// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::convert::TryFrom;

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#14
//# The maximum datagram size MUST be at least 1200 bytes.
pub const MINIMUM_MTU: u16 = 1200;

// TODO decide on better defaults
pub const DEFAULT_MAX_MTU: MaxMtu = MaxMtu(1500);

// Length is the length in octets of this user datagram  including  this
// header and the data. (This means the minimum value of the length is
// eight.)
// See https://tools.ietf.org/rfc/rfc768.txt
pub const UDP_HEADER_LEN: u16 = 8;

// IPv4 header ranges from 20-60 bytes, depending on Options
pub const IPV4_MIN_HEADER_LEN: u16 = 20;
// IPv6 header is always 40 bytes, plus extensions
pub const IPV6_MIN_HEADER_LEN: u16 = 40;
#[cfg(feature = "ipv6")]
const IP_MIN_HEADER_LEN: u16 = IPV6_MIN_HEADER_LEN;
#[cfg(not(feature = "ipv6"))]
const IP_MIN_HEADER_LEN: u16 = IPV4_MIN_HEADER_LEN;

// Initial PTO backoff multiplier is 1 indicating no additional increase to the backoff.
pub const INITIAL_PTO_BACKOFF: u32 = 1;

#[derive(Clone, Copy, Debug)]
pub struct MaxMtu(u16);

impl Default for MaxMtu {
    fn default() -> Self {
        DEFAULT_MAX_MTU
    }
}

impl TryFrom<u16> for MaxMtu {
    type Error = MaxMtuError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        let min_allowed = MINIMUM_MTU + UDP_HEADER_LEN + IP_MIN_HEADER_LEN;
        if value < min_allowed {
            return Err(MaxMtuError(min_allowed));
        }

        Ok(MaxMtu(value))
    }
}

impl From<MaxMtu> for usize {
    fn from(value: MaxMtu) -> Self {
        value.0 as usize
    }
}

impl From<MaxMtu> for u16 {
    fn from(value: MaxMtu) -> Self {
        value.0
    }
}

#[derive(Debug)]
pub struct MaxMtuError(pub u16);
