// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[cfg(any(test, feature = "generator"))]
use bolero_generator::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(any(test, feature = "generator"), derive(TypeGenerator))]
pub enum Mode {
    /// Loss recovery probing to detect lost packets
    LossRecoveryProbing,
    /// Maximum transmission unit probing to determine the path MTU
    MtuProbing,
    /// Path validation to verify peer address reachability
    PathValidationOnly,
    /// Normal transmission
    Normal,
}

impl Mode {
    /// Is the transmission a probe for loss recovery
    pub fn is_loss_recovery_probing(&self) -> bool {
        matches!(self, Mode::LossRecoveryProbing)
    }

    /// Is the transmission a probe for path maximum transmission unit discovery
    pub fn is_mtu_probing(&self) -> bool {
        matches!(self, Mode::MtuProbing)
    }

    /// Is the transmission a probe for path validation
    pub fn is_path_validation(&self) -> bool {
        matches!(self, Mode::PathValidationOnly)
    }

    /// Is this transmission not a probe
    pub fn is_normal(&self) -> bool {
        matches!(self, Mode::Normal)
    }
}
