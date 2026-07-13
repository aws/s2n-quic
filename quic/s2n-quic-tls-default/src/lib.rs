// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// By default s2n-tls is the default provider on unix and rustls elsewhere (e.g. Windows).
// Enabling the `s2n-tls-default` feature forces s2n-tls as the default on every target where it
// builds, which is primarily used to exercise s2n-tls on Windows.
#[cfg(all(not(unix), not(feature = "s2n-tls-default")))]
pub use s2n_quic_rustls::*;
#[cfg(any(unix, feature = "s2n-tls-default"))]
pub use s2n_quic_tls::*;
