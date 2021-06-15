// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[cfg(feature = "generator")]
use bolero_generator::*;

#[cfg(test)]
use bolero::generator::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(any(feature = "generator", test), derive(TypeGenerator))]
pub enum Constraint {
    /// Anti-amplification limits
    AmplificationLimited,
    /// Congestion controller window size
    CongestionLimited,
    /// Congestion controller fast retransmission
    RetransmissionOnly,
    /// No constraints
    None,
}

impl Constraint {
    /// True if the transmission is constrained by anti-amplification limits
    pub fn is_amplification_limited(self) -> bool {
        matches!(self, Self::AmplificationLimited)
    }

    /// True if the transmission is constrained by congestion controller window size
    pub fn is_congestion_limited(self) -> bool {
        matches!(self, Self::CongestionLimited)
    }

    /// True if the transmission is constrained to only retransmissions due to the congestion
    /// controller being in the fast retransmission state
    pub fn is_retransmission_only(self) -> bool {
        matches!(self, Self::RetransmissionOnly)
    }

    /// True if new data can be transmitted
    pub fn can_transmit(self) -> bool {
        self.is_none()
    }

    /// True if lost data can be retransmitted
    pub fn can_retransmit(self) -> bool {
        self.can_transmit() || self.is_retransmission_only()
    }

    /// True if there are no constraints
    fn is_none(self) -> bool {
        matches!(self, Self::None)
    }
}
