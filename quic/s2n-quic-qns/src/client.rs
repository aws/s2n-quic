// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[cfg(feature = "interop")]
mod h09;
#[cfg(feature = "interop")]
mod h3;
#[cfg(feature = "interop")]
pub mod interop;
#[cfg(feature = "perf")]
pub mod perf;

#[cfg(feature = "interop")]
pub use interop::Interop;
#[cfg(feature = "perf")]
pub use perf::Perf;

use crate::{
    congestion_control::{CongestionControl, CongestionController::*},
    tls,
    tls::TlsProviders::*,
    Result,
};
use s2n_quic::{
    client,
    client::ClientProviders,
    provider::congestion_controller::{Bbr, Cubic},
};

/// Build and start a client with the given TLS configuration and Congestion Controller
pub fn build(
    builder: client::Builder<impl ClientProviders>,
    alpns: &[String],
    tls_client: &tls::Client,
    congestion_control: &CongestionControl,
) -> Result<s2n_quic::Client> {
    macro_rules! build {
        ($build_tls:ident, $cc:ident $(, $alpns:ident)?) => {
            {
                let tls = tls_client.$build_tls($($alpns)?)?;

                builder
                    .with_tls(tls)?
                    .with_congestion_controller($cc::default())?
                    .start()
                    .unwrap()
            }
        }
    }

    Ok(
        match (tls_client.tls, congestion_control.congestion_controller) {
            #[cfg(unix)]
            (S2N, Cubic) => build!(build_s2n_tls, Cubic, alpns),
            #[cfg(unix)]
            (S2N, Bbr) => build!(build_s2n_tls, Bbr, alpns),
            (Rustls, Cubic) => build!(build_rustls, Cubic, alpns),
            (Rustls, Bbr) => build!(build_rustls, Bbr, alpns),
            (Null, Cubic) => build!(build_null, Cubic),
            (Null, Bbr) => build!(build_null, Bbr),
        },
    )
}
