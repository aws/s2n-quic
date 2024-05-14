// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection::Limits,
    crypto::tls::TlsSession,
    event::{api::SocketAddress, IntoEvent as _},
    inet,
    path::MaxMtu,
    stateless_reset,
    transport::parameters::InitialFlowControlLimits,
    varint::VarInt,
};
use core::time::Duration;

pub use disabled::*;

mod disabled;

// dc versions supported by this code, in order of preference (SUPPORTED_VERSIONS[0] is most preferred)
const SUPPORTED_VERSIONS: &[u32] = &[0x0];

/// Called on the server to select the dc version to use (if any)
///
/// The server's version preference takes precedence
pub fn select_version(client_supported_versions: &[u32]) -> Option<u32> {
    SUPPORTED_VERSIONS
        .iter()
        .find(|&supported_version| client_supported_versions.contains(supported_version))
        .copied()
}

/// The `dc::Endpoint` trait provides a way to support dc functionality
pub trait Endpoint: 'static + Send {
    /// If enabled, a dc version will attempt to be negotiated and dc-specific frames
    /// will be processed. Otherwise, no dc version will be negotiated and dc-specific
    /// frames received will result in a connection error.
    const ENABLED: bool = true;

    type Path: Path;

    /// Called when a dc version has been negotiated for the given `ConnectionInfo`
    fn new_path(&mut self, connection_info: &ConnectionInfo) -> Self::Path;
}

/// A dc path
pub trait Path: 'static + Send {
    /// Called when path secrets are ready to be derived from the given `TlsSession`
    fn on_path_secrets_ready(&mut self, session: &impl TlsSession);

    /// Called when a `DC_STATELESS_RESET_TOKENS` frame has been received from the peer
    fn on_peer_stateless_reset_tokens<'a>(
        &mut self,
        stateless_reset_tokens: impl Iterator<Item = &'a stateless_reset::Token>,
    );

    /// Returns the stateless reset tokens to include in a `DC_STATELESS_RESET_TOKENS`
    /// frame sent to the peer.
    fn stateless_reset_tokens(&mut self) -> &[stateless_reset::Token];
}

impl<P: Path> Path for Option<P> {
    #[inline]
    fn on_path_secrets_ready(&mut self, session: &impl TlsSession) {
        if let Some(path) = self {
            path.on_path_secrets_ready(session)
        }
    }

    #[inline]
    fn on_peer_stateless_reset_tokens<'a>(
        &mut self,
        stateless_reset_tokens: impl Iterator<Item = &'a stateless_reset::Token>,
    ) {
        if let Some(path) = self {
            path.on_peer_stateless_reset_tokens(stateless_reset_tokens)
        }
    }

    #[inline]
    fn stateless_reset_tokens(&mut self) -> &[stateless_reset::Token] {
        if let Some(path) = self {
            path.stateless_reset_tokens()
        } else {
            &[]
        }
    }
}

/// Information about the connection that may be used
/// when create a new dc path
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ConnectionInfo<'a> {
    /// The address (IP + Port) of the remote peer
    pub remote_address: SocketAddress<'a>,
    /// The dc version that has been negotiated
    pub dc_version: u32,
    /// Various settings relevant to the dc path
    pub application_params: ApplicationParams,
}

impl<'a> ConnectionInfo<'a> {
    #[inline]
    #[doc(hidden)]
    pub fn new(
        remote_address: &'a inet::SocketAddress,
        dc_version: u32,
        application_params: ApplicationParams,
    ) -> Self {
        Self {
            remote_address: remote_address.into_event(),
            dc_version,
            application_params,
        }
    }
}

/// Various settings relevant to the dc path
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ApplicationParams {
    pub max_mtu: MaxMtu,
    pub remote_max_data: VarInt,
    pub local_send_max_data: VarInt,
    pub local_recv_max_data: VarInt,
    pub max_idle_timeout: Option<Duration>,
    pub max_ack_delay: Duration,
}

impl ApplicationParams {
    pub fn new(
        max_mtu: MaxMtu,
        peer_flow_control_limits: &InitialFlowControlLimits,
        limits: &Limits,
    ) -> Self {
        Self {
            max_mtu,
            remote_max_data: peer_flow_control_limits.max_data,
            local_send_max_data: limits.initial_stream_limits().max_data_bidi_local,
            local_recv_max_data: limits.initial_stream_limits().max_data_bidi_remote,
            max_idle_timeout: limits.max_idle_timeout(),
            max_ack_delay: limits.max_ack_delay.into(),
        }
    }
}
