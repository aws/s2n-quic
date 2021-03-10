// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::fmt;

/// A placeholder for `PacketNumber` values.
///
/// This is used to preserve the size of partially decoded
/// packets, before packet protection is removed, and fully decoded
/// packets, after packet protection is removed.
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProtectedPacketNumber;

impl fmt::Debug for ProtectedPacketNumber {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ProtectedPacketNumber")
    }
}
