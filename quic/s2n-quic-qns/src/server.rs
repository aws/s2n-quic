// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

mod h09;
mod h3;
pub mod interop;
pub mod perf;
#[cfg(all(s2n_quic_unstable, feature = "unstable_client_hello"))]
mod unstable;

pub use interop::Interop;
pub use perf::Perf;

use crate::{
    congestion_control::{CongestionControl, CongestionController::*},
    tls,
    tls::TlsProviders::*,
    Result,
};
use s2n_quic::{
    provider::congestion_controller::{Bbr, Cubic},
    server,
    server::ServerProviders,
};

/// Build and start a server with the given TLS configuration and Congestion Controller
pub fn build(
    builder: server::Builder<impl ServerProviders>,
    alpns: &[String],
    tls_server: &tls::Server,
    congestion_control: &CongestionControl,
) -> Result<s2n_quic::Server> {
    macro_rules! build {
        ($build_tls:ident, $cc:ident $(, $alpns:ident)?) => {
            {
                let tls = tls_server.$build_tls($($alpns)?)?;

                builder
                    .with_tls(tls)?
                    .with_congestion_controller($cc::default())?
                    .start()
                    .unwrap()
            }
        }
    }

    Ok(
        match (tls_server.tls, congestion_control.congestion_controller) {
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
