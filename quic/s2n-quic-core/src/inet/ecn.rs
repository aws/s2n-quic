// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[cfg(feature = "generator")]
use bolero_generator::*;

//= https://www.rfc-editor.org/rfc/rfc3168#section-5
//# This document specifies that the Internet provide a congestion
//# indication for incipient congestion (as in RED and earlier work
//# [RJ90]) where the notification can sometimes be through marking
//# packets rather than dropping them.  This uses an ECN field in the IP
//# header with two bits, making four ECN codepoints, '00' to '11'.  The
//# ECN-Capable Transport (ECT) codepoints '10' and '01' are set by the
//# data sender to indicate that the end-points of the transport protocol
//# are ECN-capable; we call them ECT(0) and ECT(1) respectively.  The
//# phrase "the ECT codepoint" in this documents refers to either of the
//# two ECT codepoints.  Routers treat the ECT(0) and ECT(1) codepoints
//# as equivalent.  Senders are free to use either the ECT(0) or the
//# ECT(1) codepoint to indicate ECT, on a packet-by-packet basis.
//#
//# The use of both the two codepoints for ECT, ECT(0) and ECT(1), is
//# motivated primarily by the desire to allow mechanisms for the data
//# sender to verify that network elements are not erasing the CE
//# codepoint, and that data receivers are properly reporting to the
//# sender the receipt of packets with the CE codepoint set, as required
//# by the transport protocol.  Guidelines for the senders and receivers
//# to differentiate between the ECT(0) and ECT(1) codepoints will be
//# addressed in separate documents, for each transport protocol.  In
//# particular, this document does not address mechanisms for TCP end-
//# nodes to differentiate between the ECT(0) and ECT(1) codepoints.
//# Protocols and senders that only require a single ECT codepoint SHOULD
//# use ECT(0).
//#
//# The not-ECT codepoint '00' indicates a packet that is not using ECN.
//# The CE codepoint '11' is set by a router to indicate congestion to
//# the end nodes.  Routers that have a packet arriving at a full queue
//# drop the packet, just as they do in the absence of ECN.
//#
//#    +-----+-----+
//#    | ECN FIELD |
//#    +-----+-----+
//#      ECT   CE         [Obsolete] RFC 2481 names for the ECN bits.
//#       0     0         Not-ECT
//#       0     1         ECT(1)
//#       1     0         ECT(0)
//#       1     1         CE
//#
//#    Figure 1: The ECN Field in IP.

/// Explicit Congestion Notification
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord, Hash)]
#[cfg_attr(feature = "generator", derive(TypeGenerator))]
pub enum ExplicitCongestionNotification {
    /// The not-ECT codepoint '00' indicates a packet that is not using ECN.
    NotEct = 0b00,

    /// ECT(1) is set by the data sender to indicate that the end-points of the transport
    /// protocol are ECN-capable.
    Ect1 = 0b01,

    /// ECT(0) is set by the data sender to indicate that the end-points of the transport
    /// protocol are ECN-capable.
    /// Protocols and senders that only require a single ECT codepoint SHOULD use ECT(0).
    Ect0 = 0b10,

    /// The CE codepoint '11' is set by a router to indicate congestion to the end nodes.
    Ce = 0b11,
}

impl Default for ExplicitCongestionNotification {
    fn default() -> Self {
        Self::NotEct
    }
}

impl ExplicitCongestionNotification {
    /// Create a ExplicitCongestionNotification from the ECN field in the IP header
    pub fn new(ecn_field: u8) -> Self {
        match ecn_field & 0b11 {
            0b00 => ExplicitCongestionNotification::NotEct,
            0b01 => ExplicitCongestionNotification::Ect1,
            0b10 => ExplicitCongestionNotification::Ect0,
            0b11 => ExplicitCongestionNotification::Ce,
            _ => unreachable!(),
        }
    }

    /// Returns true if congestion was experienced by the peer
    pub fn congestion_experienced(self) -> bool {
        self == Self::Ce
    }

    /// Returns true if ECN is in use
    pub fn using_ecn(self) -> bool {
        self != Self::NotEct
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new() {
        for ecn in &[
            ExplicitCongestionNotification::NotEct,
            ExplicitCongestionNotification::Ect1,
            ExplicitCongestionNotification::Ect0,
            ExplicitCongestionNotification::Ce,
        ] {
            assert_eq!(*ecn, ExplicitCongestionNotification::new(*ecn as u8));
        }
    }

    /// The most-significant 6 bits of the 8-bit traffic class field ECN markings are
    /// read from are used for the differentiated services code point. This test
    /// ensures we still parse ECN bits correctly even when the DSCP markings are present.
    #[test]
    fn dscp_markings() {
        for i in 0..u8::MAX {
            for ecn in &[
                ExplicitCongestionNotification::NotEct,
                ExplicitCongestionNotification::Ect1,
                ExplicitCongestionNotification::Ect0,
                ExplicitCongestionNotification::Ce,
            ] {
                assert_eq!(
                    *ecn,
                    ExplicitCongestionNotification::new(i << 2 | *ecn as u8)
                );
            }
        }
    }
}
