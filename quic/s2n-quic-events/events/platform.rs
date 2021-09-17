// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[event("platform:tx")]
#[subject(endpoint)]
/// Emitted when the platform sends at least one packet
struct PlatformTx {
    /// The number of packets sent
    count: usize,
}

#[event("platform:tx_error")]
#[subject(endpoint)]
/// Emitted when the platform returns an error while sending datagrams
struct PlatformTxError {
    /// The error code returned by the platform
    errno: i32,
}

#[cfg(feature = "std")]
impl From<PlatformTxError> for std::io::Error {
    fn from(error: PlatformTxError) -> Self {
        Self::from_raw_os_error(error.errno)
    }
}

#[event("platform:rx")]
#[subject(endpoint)]
/// Emitted when the platform receives at least one packet
struct PlatformRx {
    /// The number of packets received
    count: usize,
}

#[event("platform:rx_error")]
#[subject(endpoint)]
/// Emitted when the platform returns an error while receiving datagrams
struct PlatformRxError {
    /// The error code returned by the platform
    errno: i32,
}

#[cfg(feature = "std")]
impl From<PlatformRxError> for std::io::Error {
    fn from(error: PlatformRxError) -> Self {
        Self::from_raw_os_error(error.errno)
    }
}

#[event("platform:gso_disabled")]
#[subject(endpoint)]
/// Emitted when GSO has been disabled due to a platform error
struct PlatformGsoDisabled {
    /// The previously configured max_segments before GSO was disabled
    previous_max_segments: usize,
    /// The number of packets that were discarded as a result of the error
    discarded_packets: usize,
}
