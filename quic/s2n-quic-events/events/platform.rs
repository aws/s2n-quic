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

#[event("platform:feature_configured")]
#[subject(endpoint)]
/// Emitted when a platform feature is configured
struct PlatformFeatureConfigured {
    configuration: PlatformFeatureConfiguration,
}

enum PlatformFeatureConfiguration {
    /// Emitted when segment offload was configured
    Gso {
        /// The maximum number of segments that can be sent in a single GSO packet
        ///
        /// If this value not greater than 1, GSO is disabled.
        max_segments: usize,
    },
    /// Emitted when receive segment offload was configured
    Gro { enabled: bool },
    /// Emitted when ECN support is configured
    Ecn { enabled: bool },
    /// Emitted when the base maximum transmission unit is configured
    BaseMtu { mtu: u16 },
    /// Emitted when the initial maximum transmission unit is configured
    InitialMtu { mtu: u16 },
    /// Emitted when the max maximum transmission unit is configured
    MaxMtu { mtu: u16 },
}

#[event("platform:event_loop_wakeup")]
#[subject(endpoint)]
struct PlatformEventLoopWakeup {
    timeout_expired: bool,
    rx_ready: bool,
    tx_ready: bool,
    application_wakeup: bool,
}

#[event("platform:event_loop_sleep")]
#[subject(endpoint)]
struct PlatformEventLoopSleep {
    /// The next time at which the event loop will wake
    timeout: Option<core::time::Duration>,
    /// The amount of time spent processing endpoint events in a single event loop
    processing_duration: core::time::Duration,
}
